use anyhow::Result;
use ara_p2p::content::ContentManager;
use ara_p2p::node::IrohNodeMem;

/// Test: Start a node, import content, verify it exists in store.
#[tokio::test]
async fn test_import_and_read_content() -> Result<()> {
    let node = IrohNodeMem::start().await?;
    let content_mgr = ContentManager::new(node.blobs_client());

    // Import some bytes
    let data = b"Hello, Ara Marketplace! This is test content.";
    let hash = content_mgr.import_bytes(data.to_vec()).await?;

    // Verify the hash is non-zero
    assert_ne!(hash, [0u8; 32], "Hash should not be all zeros");

    // Verify we can read it back
    let read_back = content_mgr.read_blob(&hash).await?;
    assert_eq!(read_back.as_ref(), data.as_slice());

    // Verify has_blob returns true
    assert!(content_mgr.has_blob(&hash).await?);

    // Verify a random hash returns false
    assert!(!content_mgr.has_blob(&[42u8; 32]).await?);

    // Drop client before shutdown to avoid RPC channel teardown races
    drop(content_mgr);
    node.shutdown().await?;
    Ok(())
}

/// Test: Two nodes — node A imports content, node B downloads it via a local relay.
#[tokio::test]
async fn test_two_node_transfer() -> Result<()> {
    use iroh::{Endpoint, NodeAddr, RelayMode};
    use iroh_blobs::net_protocol::Blobs;
    use iroh_blobs::util::local_pool::LocalPool;
    use iroh_gossip::net::Gossip;
    use std::sync::Arc;

    // Start a local relay server for the test
    let (relay_map, relay_url, _relay_guard) = iroh::test_utils::run_relay_server().await?;
    let relay_mode = RelayMode::Custom(relay_map);

    // Build node A with local relay
    let ep_a = Endpoint::builder()
        .relay_mode(relay_mode.clone())
        .insecure_skip_relay_cert_verify(true)
        .bind()
        .await?;
    let pool_a = LocalPool::default();
    let blobs_a = Blobs::memory().build(pool_a.handle(), &ep_a);
    let addr_a = ep_a.node_addr().await?;
    let gossip_a = Gossip::from_endpoint(ep_a.clone(), Default::default(), &addr_a.info);
    let router_a = iroh::protocol::Router::builder(ep_a.clone())
        .accept(iroh_blobs::ALPN, blobs_a.clone())
        .accept(iroh_gossip::ALPN, Arc::new(gossip_a))
        .spawn()
        .await?;

    // Build node B with local relay
    let ep_b = Endpoint::builder()
        .relay_mode(relay_mode)
        .insecure_skip_relay_cert_verify(true)
        .bind()
        .await?;
    let pool_b = LocalPool::default();
    let blobs_b = Blobs::memory().build(pool_b.handle(), &ep_b);
    let addr_b = ep_b.node_addr().await?;
    let gossip_b = Gossip::from_endpoint(ep_b.clone(), Default::default(), &addr_b.info);
    let router_b = iroh::protocol::Router::builder(ep_b.clone())
        .accept(iroh_blobs::ALPN, blobs_b.clone())
        .accept(iroh_gossip::ALPN, Arc::new(gossip_b))
        .spawn()
        .await?;

    let client_a = blobs_a.client();
    let client_b = blobs_b.client();

    let content_a = ContentManager::new(client_a);
    let content_b = ContentManager::new(client_b);

    // Node A imports content
    let data = b"Decentralized content for the Ara marketplace. Stake ARA, seed content, earn ETH!";
    let hash = content_a.import_bytes(data.to_vec()).await?;

    // Node B should not have it yet
    assert!(!content_b.has_blob(&hash).await?);

    // Create node A address with relay URL (needed for relay-based connection)
    let node_a_addr = NodeAddr::new(ep_a.node_id()).with_relay_url(relay_url);

    // Node B downloads from Node A via relay
    content_b.download_from(&hash, node_a_addr).await?;

    // Node B should now have it
    assert!(content_b.has_blob(&hash).await?);

    // Verify content matches
    let read_back = content_b.read_blob(&hash).await?;
    assert_eq!(read_back.as_ref(), data.as_slice());

    router_a.shutdown().await?;
    router_b.shutdown().await?;
    Ok(())
}

/// Test: Import a file from disk, export it back, verify round-trip.
#[tokio::test]
async fn test_file_import_export_roundtrip() -> Result<()> {
    let node = IrohNodeMem::start().await?;
    let content_mgr = ContentManager::new(node.blobs_client());

    // Create a temp file
    let tmp_dir = tempfile::tempdir()?;
    let input_path = tmp_dir.path().join("test_content.bin");
    let output_path = tmp_dir.path().join("exported_content.bin");

    // Write test data to input file
    let test_data = vec![0xAA; 1024 * 10]; // 10 KB of 0xAA
    tokio::fs::write(&input_path, &test_data).await?;

    // Import the file
    let hash = content_mgr.import_file(&input_path).await?;
    assert_ne!(hash, [0u8; 32]);

    // Export it to a different path
    content_mgr.export_blob(&hash, &output_path).await?;

    // Verify the exported file matches
    let exported_data = tokio::fs::read(&output_path).await?;
    assert_eq!(exported_data, test_data);

    drop(content_mgr);
    node.shutdown().await?;
    Ok(())
}

/// Test: Delete a blob from the store.
#[tokio::test]
async fn test_delete_blob() -> Result<()> {
    let node = IrohNodeMem::start().await?;
    let content_mgr = ContentManager::new(node.blobs_client());

    let hash = content_mgr.import_bytes(b"temporary content".to_vec()).await?;
    assert!(content_mgr.has_blob(&hash).await?);

    content_mgr.delete_blob(&hash).await?;

    // Allow iroh's internal GC / channel cleanup to settle after delete
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    drop(content_mgr);
    node.shutdown().await?;
    Ok(())
}

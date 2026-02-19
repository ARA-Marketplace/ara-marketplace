import { Routes, Route } from "react-router-dom";
import Layout from "./components/Layout";
import Marketplace from "./pages/Marketplace";
import ContentDetail from "./pages/ContentDetail";
import Publish from "./pages/Publish";
import Library from "./pages/Library";
import Dashboard from "./pages/Dashboard";
import Wallet from "./pages/Wallet";

function App() {
  return (
    <Layout>
      <Routes>
        <Route path="/" element={<Marketplace />} />
        <Route path="/content/:contentId" element={<ContentDetail />} />
        <Route path="/publish" element={<Publish />} />
        <Route path="/library" element={<Library />} />
        <Route path="/dashboard" element={<Dashboard />} />
        <Route path="/wallet" element={<Wallet />} />
      </Routes>
    </Layout>
  );
}

export default App;

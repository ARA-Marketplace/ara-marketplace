import type { ContentType } from "./types";

export const CATEGORIES_BY_TYPE: Record<ContentType, string[]> = {
  game: [
    "Action", "Adventure", "RPG", "Strategy", "Puzzle", "Simulation",
    "Sports", "Racing", "Fighting", "Horror", "Platformer", "Shooter",
    "Survival", "Sandbox", "Card Game", "Tower Defense", "Visual Novel",
    "Indie", "Educational", "Multiplayer", "Casual",
  ],
  music: [
    "Pop", "Rock", "Hip-Hop", "Electronic", "Jazz", "Classical",
    "R&B", "Country", "Folk", "Metal", "Punk", "Blues",
    "Reggae", "Latin", "Ambient", "Lo-Fi", "Soundtrack",
    "Experimental", "Instrumental", "Podcast",
  ],
  video: [
    "Film", "Short Film", "Documentary", "Animation", "Music Video",
    "Tutorial", "Vlog", "Comedy", "Drama", "Thriller", "Horror",
    "Sci-Fi", "Action", "Sports", "Nature", "Travel", "Cooking",
    "Gaming", "Livestream", "Behind the Scenes",
  ],
  document: [
    "eBook", "Research Paper", "Tutorial", "Guide", "Whitepaper",
    "Magazine", "Newsletter", "Comic", "Manga", "Technical Manual",
    "Textbook", "Recipe", "Template", "Cheat Sheet", "Reference",
    "Case Study", "Thesis", "Report",
  ],
  software: [
    "Utility", "Productivity", "Developer Tool", "Plugin", "Extension",
    "Theme", "Library", "Framework", "CLI Tool", "Desktop App",
    "Mobile App", "Game Mod", "Script", "Bot", "AI Model",
    "Dataset", "Font", "Emulator",
  ],
  other: [
    "3D Model", "Texture Pack", "Sound Effect", "Sample Pack", "Preset",
    "Template", "Wallpaper", "Icon Pack", "Photography", "Digital Art",
    "NFT Collectible", "Miscellaneous",
  ],
};

/** Flat list of all categories across all types (for search/filtering) */
export const ALL_CATEGORIES: string[] = [
  ...new Set(Object.values(CATEGORIES_BY_TYPE).flat()),
];

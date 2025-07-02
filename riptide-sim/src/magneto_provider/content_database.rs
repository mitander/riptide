//! Default content database for mock torrent discovery.

use super::MockTorrentEntry;

/// Creates default content database with realistic torrent entries.
pub fn create_default_content_database() -> Vec<(String, Vec<MockTorrentEntry>)> {
    let mut entries = Vec::new();

    // Open source movies
    let open_source_movies = vec![
        MockTorrentEntry {
            name: "Big Buck Bunny (2008) [1080p]".to_string(),
            size_bytes: 1_500_000_000, // 1.5GB
            seeders: 245,
            leechers: 23,
            magnet_template: "magnet:?xt=urn:btih:BIG_BUCK_BUNNY_HASH".to_string(),
            categories: vec!["movie".to_string(), "1080p".to_string()],
        },
        MockTorrentEntry {
            name: "Sintel (2010) [4K]".to_string(),
            size_bytes: 8_500_000_000, // 8.5GB
            seeders: 156,
            leechers: 45,
            magnet_template: "magnet:?xt=urn:btih:SINTEL_4K_HASH".to_string(),
            categories: vec!["movie".to_string(), "4k".to_string()],
        },
        MockTorrentEntry {
            name: "Tears of Steel (2012) [720p]".to_string(),
            size_bytes: 750_000_000, // 750MB
            seeders: 89,
            leechers: 12,
            magnet_template: "magnet:?xt=urn:btih:TEARS_OF_STEEL_HASH".to_string(),
            categories: vec!["movie".to_string(), "720p".to_string()],
        },
        MockTorrentEntry {
            name: "Elephants Dream (2006) [1080p]".to_string(),
            size_bytes: 1_200_000_000, // 1.2GB
            seeders: 67,
            leechers: 8,
            magnet_template: "magnet:?xt=urn:btih:ELEPHANTS_DREAM_HASH".to_string(),
            categories: vec!["movie".to_string(), "1080p".to_string()],
        },
    ];

    // Creative Commons content
    let creative_commons = vec![
        MockTorrentEntry {
            name: "Creative Commons Movie Collection [Mixed Quality]".to_string(),
            size_bytes: 25_000_000_000, // 25GB
            seeders: 423,
            leechers: 156,
            magnet_template: "magnet:?xt=urn:btih:CC_COLLECTION_HASH".to_string(),
            categories: vec!["collection".to_string(), "creative_commons".to_string()],
        },
        MockTorrentEntry {
            name: "Internet Archive Documentary Pack".to_string(),
            size_bytes: 45_000_000_000, // 45GB
            seeders: 234,
            leechers: 67,
            magnet_template: "magnet:?xt=urn:btih:IA_DOCS_HASH".to_string(),
            categories: vec!["documentary".to_string(), "collection".to_string()],
        },
    ];

    // Linux distributions
    let linux_distros = vec![
        MockTorrentEntry {
            name: "Ubuntu 22.04.3 Desktop amd64".to_string(),
            size_bytes: 4_700_000_000, // 4.7GB
            seeders: 1245,
            leechers: 234,
            magnet_template: "magnet:?xt=urn:btih:UBUNTU_22_04_HASH".to_string(),
            categories: vec!["software".to_string(), "linux".to_string()],
        },
        MockTorrentEntry {
            name: "Debian 12.2.0 amd64 DVD".to_string(),
            size_bytes: 3_900_000_000, // 3.9GB
            seeders: 567,
            leechers: 89,
            magnet_template: "magnet:?xt=urn:btih:DEBIAN_12_HASH".to_string(),
            categories: vec!["software".to_string(), "linux".to_string()],
        },
    ];

    // Games and software
    let games_software = vec![
        MockTorrentEntry {
            name: "OpenTTD 13.4 Full Game Collection".to_string(),
            size_bytes: 2_100_000_000, // 2.1GB
            seeders: 145,
            leechers: 34,
            magnet_template: "magnet:?xt=urn:btih:OPENTTD_HASH".to_string(),
            categories: vec!["game".to_string(), "open_source".to_string()],
        },
        MockTorrentEntry {
            name: "Blender 4.0 Complete Suite + Assets".to_string(),
            size_bytes: 12_000_000_000, // 12GB
            seeders: 234,
            leechers: 78,
            magnet_template: "magnet:?xt=urn:btih:BLENDER_4_HASH".to_string(),
            categories: vec!["software".to_string(), "creative".to_string()],
        },
    ];

    // Add category collections
    entries.push(("open source movies".to_string(), open_source_movies.clone()));
    entries.push(("creative commons".to_string(), creative_commons));
    entries.push(("linux".to_string(), linux_distros.clone()));
    entries.push(("games".to_string(), games_software.clone()));
    entries.push(("software".to_string(), games_software.clone()));

    // Add individual movie mappings
    entries.push((
        "big buck bunny".to_string(),
        vec![open_source_movies[0].clone()],
    ));
    entries.push(("sintel".to_string(), vec![open_source_movies[1].clone()]));
    entries.push((
        "tears of steel".to_string(),
        vec![open_source_movies[2].clone()],
    ));
    entries.push((
        "elephants dream".to_string(),
        vec![open_source_movies[3].clone()],
    ));

    // Add individual software/distro mappings
    entries.push(("ubuntu".to_string(), vec![linux_distros[0].clone()]));
    entries.push(("debian".to_string(), vec![linux_distros[1].clone()]));
    entries.push(("blender".to_string(), vec![games_software[1].clone()]));

    entries
}

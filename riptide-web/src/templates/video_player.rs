//! Video player page template

/// Generates the video player page content
pub fn video_player_content(info_hash: &str, is_local: bool) -> String {
    const VIDEO_PLAYER_TEMPLATE: &str = include_str!("../../templates/video_player.html");

    VIDEO_PLAYER_TEMPLATE
        .replace("{{ info_hash }}", info_hash)
        .replace(
            "{{ local_param }}",
            if is_local { "?local=true" } else { "" },
        )
        .replace(
            "{{ source_type }}",
            if is_local {
                "Local File"
            } else {
                "BitTorrent Stream"
            },
        )
}

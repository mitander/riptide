//! MP4 validation and streaming compatibility integration tests
//!
//! Tests the complete MP4 validation pipeline including format detection,
//! structure analysis, and browser compatibility validation. Verifies that
//! the remuxing pipeline produces streaming-compatible MP4 files.

use std::io::Cursor;

use riptide_core::streaming::mp4_validation::{
    analyze_mp4_for_streaming, test_remuxed_mp4_validity, validate_mp4_structure,
};

/// Creates a valid streaming-compatible MP4 header for testing
fn create_streaming_mp4_header() -> Vec<u8> {
    let mut mp4_data = Vec::new();

    // File Type Box (ftyp) - must come first for streaming
    mp4_data.extend_from_slice(&[0x00, 0x00, 0x00, 0x20]); // size = 32 bytes
    mp4_data.extend_from_slice(b"ftyp"); // type
    mp4_data.extend_from_slice(b"isom"); // major brand
    mp4_data.extend_from_slice(&[0x00, 0x00, 0x02, 0x00]); // minor version
    mp4_data.extend_from_slice(b"isom"); // compatible brands
    mp4_data.extend_from_slice(b"iso2");
    mp4_data.extend_from_slice(b"avc1");
    mp4_data.extend_from_slice(b"mp41");

    // Movie Box (moov) - must come before mdat for streaming
    mp4_data.extend_from_slice(&[0x00, 0x00, 0x00, 0x08]); // size = 8 bytes (minimal)
    mp4_data.extend_from_slice(b"moov"); // type

    // Media Data Box (mdat) - after moov for streaming compatibility
    mp4_data.extend_from_slice(&[0x00, 0x00, 0x00, 0x08]); // size = 8 bytes (minimal)
    mp4_data.extend_from_slice(b"mdat"); // type

    mp4_data
}

/// Creates a non-streaming MP4 with problematic structure
fn create_non_streaming_mp4_header() -> Vec<u8> {
    let mut mp4_data = Vec::new();

    // File Type Box (ftyp)
    mp4_data.extend_from_slice(&[0x00, 0x00, 0x00, 0x20]); // size = 32 bytes
    mp4_data.extend_from_slice(b"ftyp"); // type
    mp4_data.extend_from_slice(b"isom"); // major brand
    mp4_data.extend_from_slice(&[0x00, 0x00, 0x02, 0x00]); // minor version
    mp4_data.extend_from_slice(b"isom"); // compatible brands
    mp4_data.extend_from_slice(b"iso2");
    mp4_data.extend_from_slice(b"avc1");
    mp4_data.extend_from_slice(b"mp41");

    // Media Data Box (mdat) BEFORE moov - bad for streaming
    mp4_data.extend_from_slice(&[0x00, 0x00, 0x00, 0x08]); // size = 8 bytes
    mp4_data.extend_from_slice(b"mdat"); // type

    // Movie Box (moov) AFTER mdat - requires seeking, bad for streaming
    mp4_data.extend_from_slice(&[0x00, 0x00, 0x00, 0x08]); // size = 8 bytes
    mp4_data.extend_from_slice(b"moov"); // type

    mp4_data
}

/// Creates fragmented MP4 header (also streaming-compatible)
fn create_fragmented_mp4_header() -> Vec<u8> {
    let mut mp4_data = Vec::new();

    // File Type Box with fragmented brand
    mp4_data.extend_from_slice(&[0x00, 0x00, 0x00, 0x20]); // size = 32 bytes
    mp4_data.extend_from_slice(b"ftyp"); // type
    mp4_data.extend_from_slice(b"isom"); // major brand
    mp4_data.extend_from_slice(&[0x00, 0x00, 0x02, 0x00]); // minor version
    mp4_data.extend_from_slice(b"isom"); // compatible brands
    mp4_data.extend_from_slice(b"dash");
    mp4_data.extend_from_slice(b"mp41");
    mp4_data.extend_from_slice(b"frag");

    // Movie Box (moov) with minimal initialization
    mp4_data.extend_from_slice(&[0x00, 0x00, 0x00, 0x08]); // size = 8 bytes
    mp4_data.extend_from_slice(b"moov"); // type

    // Movie Fragment Box (moof) - for fragmented MP4
    mp4_data.extend_from_slice(&[0x00, 0x00, 0x00, 0x08]); // size = 8 bytes
    mp4_data.extend_from_slice(b"moof"); // type

    mp4_data
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_streaming_mp4_validation_detects_good_structure() {
        let streaming_mp4 = create_streaming_mp4_header();
        let analysis = analyze_mp4_for_streaming(&streaming_mp4);

        assert!(
            analysis.is_streaming_ready,
            "Properly structured MP4 should be streaming-ready"
        );
        assert!(
            analysis.issues.is_empty(),
            "Valid streaming MP4 should have no issues, got: {:?}",
            analysis.issues
        );
        assert!(
            analysis.moov_position.is_some(),
            "Should detect moov box position"
        );
        assert!(
            analysis.mdat_position.is_some(),
            "Should detect mdat box position"
        );

        // Verify moov comes before mdat
        let moov_pos = analysis.moov_position.unwrap();
        let mdat_pos = analysis.mdat_position.unwrap();
        assert!(
            moov_pos < mdat_pos,
            "moov box should come before mdat box for streaming"
        );
    }

    #[test]
    fn test_streaming_mp4_validation_detects_bad_structure() {
        let non_streaming_mp4 = create_non_streaming_mp4_header();
        let analysis = analyze_mp4_for_streaming(&non_streaming_mp4);

        assert!(
            !analysis.is_streaming_ready,
            "MP4 with mdat before moov should not be streaming-ready"
        );
        assert!(
            !analysis.issues.is_empty(),
            "Problematic MP4 should have issues detected"
        );

        // Should detect the ordering issue
        let has_ordering_issue = analysis
            .issues
            .iter()
            .any(|issue| issue.contains("moov") && issue.contains("mdat"));
        assert!(
            has_ordering_issue,
            "Should detect moov/mdat ordering issue in problems: {:?}",
            analysis.issues
        );
    }

    #[test]
    fn test_fragmented_mp4_validation() {
        let fragmented_mp4 = create_fragmented_mp4_header();
        let analysis = analyze_mp4_for_streaming(&fragmented_mp4);

        // Fragmented MP4s are streaming-compatible by design
        assert!(
            analysis.is_streaming_ready,
            "Fragmented MP4 should be streaming-ready"
        );
    }

    #[test]
    fn test_mp4_structure_validation_integration() {
        let streaming_mp4 = create_streaming_mp4_header();
        let mut cursor = Cursor::new(&streaming_mp4);

        let structure = validate_mp4_structure(&mut cursor)
            .expect("Should successfully parse valid MP4 structure");

        assert!(structure.has_ftyp, "Should detect ftyp box");
        assert!(structure.has_moov, "Should detect moov box");
        assert!(structure.has_mdat, "Should detect mdat box");
        assert!(
            structure.moov_before_mdat,
            "Should correctly identify moov before mdat"
        );
        assert!(
            structure.is_streaming_compatible(),
            "Should identify as streaming compatible"
        );

        let issues = structure.get_issues();
        assert!(
            issues.is_empty(),
            "Valid structure should have no issues: {issues:?}"
        );
    }

    #[test]
    fn test_mp4_structure_validation_detects_problems() {
        let non_streaming_mp4 = create_non_streaming_mp4_header();
        let mut cursor = Cursor::new(&non_streaming_mp4);

        let structure = validate_mp4_structure(&mut cursor)
            .expect("Should successfully parse MP4 structure even if problematic");

        assert!(structure.has_ftyp, "Should detect ftyp box");
        assert!(structure.has_moov, "Should detect moov box");
        assert!(structure.has_mdat, "Should detect mdat box");
        assert!(
            !structure.moov_before_mdat,
            "Should correctly identify mdat before moov"
        );
        assert!(
            !structure.is_streaming_compatible(),
            "Should identify as not streaming compatible"
        );

        let issues = structure.get_issues();
        assert!(
            !issues.is_empty(),
            "Problematic structure should have issues"
        );
        assert!(
            issues.iter().any(|issue| issue.contains("moov")),
            "Should mention moov box issue: {issues:?}"
        );
    }

    #[test]
    fn test_remuxed_mp4_validity_function() {
        // Test with good MP4
        let good_mp4 = create_streaming_mp4_header();
        let result = test_remuxed_mp4_validity(&good_mp4);
        assert!(
            result.is_ok(),
            "Valid streaming MP4 should pass validation: {:?}",
            result.err()
        );

        // Test with bad MP4
        let bad_mp4 = create_non_streaming_mp4_header();
        let result = test_remuxed_mp4_validity(&bad_mp4);
        assert!(result.is_err(), "Non-streaming MP4 should fail validation");

        if let Err(error_msg) = result {
            assert!(
                error_msg.contains("streaming") || error_msg.contains("moov"),
                "Error message should explain streaming compatibility issue: {error_msg}"
            );
        }
    }

    #[test]
    fn test_edge_cases_empty_and_truncated_data() {
        // Empty data
        let empty_data = vec![];
        let analysis = analyze_mp4_for_streaming(&empty_data);
        assert!(
            !analysis.is_streaming_ready,
            "Empty data should not be streaming-ready"
        );

        // Truncated data (too small to contain valid boxes)
        let truncated_data = vec![0x00, 0x01, 0x02, 0x03];
        let analysis = analyze_mp4_for_streaming(&truncated_data);
        assert!(
            !analysis.is_streaming_ready,
            "Truncated data should not be streaming-ready"
        );

        // Invalid box size
        let invalid_box = vec![0xFF, 0xFF, 0xFF, 0xFF, b'f', b't', b'y', b'p'];
        let analysis = analyze_mp4_for_streaming(&invalid_box);
        assert!(
            !analysis.is_streaming_ready,
            "Invalid box data should not be streaming-ready"
        );
    }

    #[test]
    fn test_browser_compatibility_validation_comprehensive() {
        let test_cases = vec![
            (
                "Valid streaming MP4",
                create_streaming_mp4_header(),
                true,
                "Should support standard streaming MP4",
            ),
            (
                "Fragmented MP4",
                create_fragmented_mp4_header(),
                true,
                "Should support fragmented MP4",
            ),
            (
                "Non-streaming MP4",
                create_non_streaming_mp4_header(),
                false,
                "Should reject non-streaming MP4",
            ),
            ("Empty data", vec![], false, "Should reject empty data"),
            (
                "Truncated data",
                vec![0x00, 0x01, 0x02],
                false,
                "Should reject truncated data",
            ),
        ];

        for (name, data, should_pass, description) in test_cases {
            let result = test_remuxed_mp4_validity(&data);

            if should_pass {
                assert!(
                    result.is_ok(),
                    "{}: {} - got error: {:?}",
                    name,
                    description,
                    result.err()
                );
            } else {
                assert!(
                    result.is_err(),
                    "{name}: {description} - should have failed but passed"
                );
            }
        }
    }

    #[test]
    fn test_mp4_analysis_provides_detailed_information() {
        let streaming_mp4 = create_streaming_mp4_header();
        let analysis = analyze_mp4_for_streaming(&streaming_mp4);

        // Should provide specific position information
        assert!(
            analysis.moov_position.is_some(),
            "Should provide moov box position"
        );
        assert!(
            analysis.mdat_position.is_some(),
            "Should provide mdat box position"
        );

        let moov_pos = analysis.moov_position.unwrap();
        let mdat_pos = analysis.mdat_position.unwrap();

        // Positions should be reasonable (within the test data)
        assert!(
            (moov_pos as usize) < streaming_mp4.len(),
            "moov position should be within file"
        );
        assert!(
            (mdat_pos as usize) < streaming_mp4.len(),
            "mdat position should be within file"
        );

        // For our test data, moov should come after ftyp (32 bytes) and before mdat
        assert!(moov_pos >= 32, "moov should come after ftyp box");
        assert!(moov_pos < mdat_pos, "moov should come before mdat");
    }

    #[test]
    fn test_validation_error_messages_are_descriptive() {
        let bad_mp4 = create_non_streaming_mp4_header();
        let result = test_remuxed_mp4_validity(&bad_mp4);

        match result {
            Err(error_msg) => {
                // Error message should be descriptive and helpful
                assert!(
                    error_msg.len() > 10,
                    "Error message should be descriptive, got: '{error_msg}'"
                );

                // Should mention key concepts
                let mentions_streaming = error_msg.to_lowercase().contains("streaming");
                let mentions_compatibility = error_msg.to_lowercase().contains("compatible")
                    || error_msg.to_lowercase().contains("compatibility");
                let mentions_structure = error_msg.to_lowercase().contains("structure")
                    || error_msg.to_lowercase().contains("moov")
                    || error_msg.to_lowercase().contains("mdat");

                assert!(
                    mentions_streaming || mentions_compatibility || mentions_structure,
                    "Error message should mention streaming, compatibility, or structure issues: '{error_msg}'"
                );
            }
            Ok(_) => panic!("Expected validation to fail for non-streaming MP4"),
        }
    }
}

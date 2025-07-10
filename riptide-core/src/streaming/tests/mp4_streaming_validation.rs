//! MP4 streaming validation tests
//!
//! Tests to validate MP4 output from remuxing pipeline and catch
//! streaming compatibility issues before they reach the browser.

use crate::streaming::mp4_validation::{
    analyze_mp4_for_streaming, test_remuxed_mp4_validity, validate_mp4_structure,
};

/// Creates a valid MP4 header for testing
fn create_test_mp4_header() -> Vec<u8> {
    let mut mp4_data = Vec::new();

    // ftyp box
    mp4_data.extend_from_slice(&[0x00, 0x00, 0x00, 0x20]); // size = 32 bytes
    mp4_data.extend_from_slice(b"ftyp"); // type
    mp4_data.extend_from_slice(b"isom"); // major brand
    mp4_data.extend_from_slice(&[0x00, 0x00, 0x02, 0x00]); // minor version
    mp4_data.extend_from_slice(b"isom"); // compatible brand
    mp4_data.extend_from_slice(b"iso2"); // compatible brand
    mp4_data.extend_from_slice(b"avc1"); // compatible brand
    mp4_data.extend_from_slice(b"mp41"); // compatible brand

    // moov box
    mp4_data.extend_from_slice(&[0x00, 0x00, 0x00, 0x08]); // size = 8 bytes (minimal)
    mp4_data.extend_from_slice(b"moov"); // type

    // mdat box
    mp4_data.extend_from_slice(&[0x00, 0x00, 0x00, 0x08]); // size = 8 bytes (minimal)
    mp4_data.extend_from_slice(b"mdat"); // type

    mp4_data
}

/// Creates an MP4 with moov after mdat (not streaming-friendly)
fn create_bad_mp4_header() -> Vec<u8> {
    let mut mp4_data = Vec::new();

    // ftyp box
    mp4_data.extend_from_slice(&[0x00, 0x00, 0x00, 0x20]); // size = 32 bytes
    mp4_data.extend_from_slice(b"ftyp"); // type
    mp4_data.extend_from_slice(b"isom"); // major brand
    mp4_data.extend_from_slice(&[0x00, 0x00, 0x02, 0x00]); // minor version
    mp4_data.extend_from_slice(b"isom"); // compatible brand
    mp4_data.extend_from_slice(b"iso2"); // compatible brand
    mp4_data.extend_from_slice(b"avc1"); // compatible brand
    mp4_data.extend_from_slice(b"mp41"); // compatible brand

    // mdat box first (bad for streaming)
    mp4_data.extend_from_slice(&[0x00, 0x00, 0x00, 0x08]); // size = 8 bytes
    mp4_data.extend_from_slice(b"mdat"); // type

    // moov box after mdat (bad for streaming)
    mp4_data.extend_from_slice(&[0x00, 0x00, 0x00, 0x08]); // size = 8 bytes
    mp4_data.extend_from_slice(b"moov"); // type

    mp4_data
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mp4_streaming_validation_good() {
        let good_mp4 = create_test_mp4_header();
        let analysis = analyze_mp4_for_streaming(&good_mp4);

        assert!(
            analysis.is_streaming_ready,
            "Valid MP4 should be streaming-ready"
        );
        assert!(
            analysis.issues.is_empty(),
            "Valid MP4 should have no issues"
        );
        assert!(analysis.moov_position.is_some(), "Should detect moov box");
        assert!(analysis.mdat_position.is_some(), "Should detect mdat box");
    }

    #[test]
    fn test_mp4_streaming_validation_bad() {
        let bad_mp4 = create_bad_mp4_header();
        let analysis = analyze_mp4_for_streaming(&bad_mp4);

        assert!(
            !analysis.is_streaming_ready,
            "Bad MP4 should not be streaming-ready"
        );
        assert!(!analysis.issues.is_empty(), "Bad MP4 should have issues");
        assert!(
            analysis.issues.iter().any(|issue| issue.contains("moov")),
            "Should detect moov box ordering issue"
        );
    }

    #[test]
    fn test_remuxed_mp4_validity_function() {
        let good_mp4 = create_test_mp4_header();
        let result = test_remuxed_mp4_validity(&good_mp4);
        assert!(result.is_ok(), "Valid MP4 should pass validation");

        let bad_mp4 = create_bad_mp4_header();
        let result = test_remuxed_mp4_validity(&bad_mp4);
        assert!(result.is_err(), "Bad MP4 should fail validation");
    }

    #[test]
    fn test_mp4_structure_validation() {
        let good_mp4 = create_test_mp4_header();
        let mut cursor = std::io::Cursor::new(&good_mp4);
        let structure = validate_mp4_structure(&mut cursor).unwrap();

        assert!(
            structure.is_streaming_compatible(),
            "Should be streaming compatible"
        );
        assert!(structure.has_ftyp, "Should have ftyp box");
        assert!(structure.has_moov, "Should have moov box");
        assert!(structure.has_mdat, "Should have mdat box");
        assert!(structure.moov_before_mdat, "moov should be before mdat");
    }

    #[test]
    fn test_mp4_structure_validation_bad() {
        let bad_mp4 = create_bad_mp4_header();
        let mut cursor = std::io::Cursor::new(&bad_mp4);
        let structure = validate_mp4_structure(&mut cursor).unwrap();

        assert!(
            !structure.is_streaming_compatible(),
            "Should not be streaming compatible"
        );
        assert!(
            !structure.moov_before_mdat,
            "moov should not be before mdat"
        );

        let issues = structure.get_issues();
        assert!(!issues.is_empty(), "Should have structural issues");
        assert!(
            issues.iter().any(|issue| issue.contains("moov")),
            "Should detect moov ordering issue"
        );
    }

    #[test]
    fn test_streaming_mp4_validation_integration() {
        // Test that catches the real streaming issues
        let good_mp4 = create_test_mp4_header();
        let analysis = analyze_mp4_for_streaming(&good_mp4);

        assert!(analysis.is_streaming_ready, "Should be streaming ready");
        assert!(
            analysis.moov_position.is_some(),
            "Should detect moov position"
        );
        assert!(
            analysis.mdat_position.is_some(),
            "Should detect mdat position"
        );

        // Test validation function
        let result = test_remuxed_mp4_validity(&good_mp4);
        assert!(result.is_ok(), "Valid MP4 should pass validation");
    }

    #[test]
    fn test_browser_compatibility_validation() {
        // Test cases for common browser compatibility issues
        let test_cases = vec![
            ("Valid streaming MP4", create_test_mp4_header(), true),
            ("Invalid moov positioning", create_bad_mp4_header(), false),
            ("Empty data", vec![], false),
            ("Too small", vec![0x00, 0x01, 0x02], false),
        ];

        for (name, data, should_pass) in test_cases {
            let result = test_remuxed_mp4_validity(&data);
            if should_pass {
                assert!(result.is_ok(), "{} should pass validation", name);
            } else {
                assert!(result.is_err(), "{} should fail validation", name);
            }
        }
    }

    #[test]
    fn test_streaming_analysis_details() {
        let good_mp4 = create_test_mp4_header();
        let analysis = analyze_mp4_for_streaming(&good_mp4);

        // Check that we get proper positioning information
        assert!(
            analysis.moov_position.is_some(),
            "Should detect moov position"
        );
        assert!(
            analysis.mdat_position.is_some(),
            "Should detect mdat position"
        );

        let moov_pos = analysis.moov_position.unwrap();
        let mdat_pos = analysis.mdat_position.unwrap();

        assert!(
            moov_pos < mdat_pos,
            "moov should be before mdat for streaming"
        );
    }

    #[test]
    fn test_mp4_validation_error_messages() {
        let bad_mp4 = create_bad_mp4_header();
        let result = test_remuxed_mp4_validity(&bad_mp4);

        match result {
            Err(error_msg) => {
                assert!(
                    error_msg.contains("streaming-ready"),
                    "Error should mention streaming compatibility"
                );
                assert!(
                    error_msg.contains("moov"),
                    "Error should mention moov box issue"
                );
            }
            Ok(_) => panic!("Should have failed validation"),
        }
    }
}

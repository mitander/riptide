//! Browser compatibility testing and detection
//!
//! Implements the browser compatibility requirements from PERFORMANCE_SOLUTION.md

use std::collections::HashMap;

use axum::http::HeaderMap;

use super::{ClientCapabilities, HttpStreamingError};

/// Browser types detected from User-Agent strings
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum BrowserType {
    Chrome,
    Firefox,
    Safari,
    Edge,
    Unknown,
}

/// Video format support matrix for different browsers
#[derive(Debug, Clone)]
pub struct BrowserCapabilityMatrix {
    support_matrix: HashMap<BrowserType, ClientCapabilities>,
}

impl Default for BrowserCapabilityMatrix {
    fn default() -> Self {
        let mut matrix = HashMap::new();

        // Chrome - Excellent format support
        matrix.insert(
            BrowserType::Chrome,
            ClientCapabilities {
                supports_mp4: true,
                supports_webm: true,
                supports_hls: true,
                user_agent: "Chrome".to_string(),
            },
        );

        // Firefox - Good format support
        matrix.insert(
            BrowserType::Firefox,
            ClientCapabilities {
                supports_mp4: true,
                supports_webm: true,
                supports_hls: false, // Firefox has limited HLS support
                user_agent: "Firefox".to_string(),
            },
        );

        // Safari - Limited format support, strong HLS
        matrix.insert(
            BrowserType::Safari,
            ClientCapabilities {
                supports_mp4: true,
                supports_webm: false, // Safari doesn't support WebM well
                supports_hls: true,
                user_agent: "Safari".to_string(),
            },
        );

        // Edge - Similar to Chrome
        matrix.insert(
            BrowserType::Edge,
            ClientCapabilities {
                supports_mp4: true,
                supports_webm: true,
                supports_hls: true,
                user_agent: "Edge".to_string(),
            },
        );

        // Unknown browsers - Conservative support
        matrix.insert(
            BrowserType::Unknown,
            ClientCapabilities {
                supports_mp4: true,   // MP4 is universally supported
                supports_webm: false, // Conservative default
                supports_hls: false,  // Conservative default
                user_agent: "Unknown".to_string(),
            },
        );

        Self {
            support_matrix: matrix,
        }
    }
}

impl BrowserCapabilityMatrix {
    /// Detect browser type and capabilities from User-Agent header
    pub fn detect_browser_capabilities(&self, headers: &HeaderMap) -> ClientCapabilities {
        let user_agent = headers
            .get("user-agent")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_lowercase();

        let browser_type = self.detect_browser_type(&user_agent);

        let mut capabilities = self
            .support_matrix
            .get(&browser_type)
            .cloned()
            .unwrap_or_else(|| self.support_matrix[&BrowserType::Unknown].clone());

        // Store actual user agent string for debugging
        capabilities.user_agent = headers
            .get("user-agent")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("Unknown")
            .to_string();

        capabilities
    }

    /// Detect browser type from user agent string
    fn detect_browser_type(&self, user_agent: &str) -> BrowserType {
        if user_agent.contains("chrome") && !user_agent.contains("edg") {
            BrowserType::Chrome
        } else if user_agent.contains("firefox") {
            BrowserType::Firefox
        } else if user_agent.contains("safari") && !user_agent.contains("chrome") {
            BrowserType::Safari
        } else if user_agent.contains("edg") {
            BrowserType::Edge
        } else {
            BrowserType::Unknown
        }
    }

    /// Recommended video format for a specific browser
    pub fn preferred_format(&self, browser_type: &BrowserType) -> String {
        match browser_type {
            BrowserType::Chrome | BrowserType::Edge => {
                "video/mp4".to_string() // Best compatibility and performance
            }
            BrowserType::Firefox => {
                "video/mp4".to_string() // MP4 for broadest support
            }
            BrowserType::Safari => {
                "video/mp4".to_string() // Safari's strongest format
            }
            BrowserType::Unknown => {
                "video/mp4".to_string() // Safest fallback
            }
        }
    }

    /// Test if a browser supports a specific MIME type
    pub fn supports_mime_type(&self, browser_type: &BrowserType, mime_type: &str) -> bool {
        let capabilities = &self.support_matrix[browser_type];

        match mime_type {
            "video/mp4" | "video/mp4; codecs=\"avc1.42E01E, mp4a.40.2\"" => {
                capabilities.supports_mp4
            }
            "video/webm"
            | "video/webm; codecs=\"vp8, vorbis\""
            | "video/webm; codecs=\"vp9, opus\"" => capabilities.supports_webm,
            "application/x-mpegURL" | "application/vnd.apple.mpegurl" => capabilities.supports_hls,
            _ => false,
        }
    }
}

/// Browser compatibility test suite
pub struct BrowserCompatibilityTester {
    capability_matrix: BrowserCapabilityMatrix,
}

impl Default for BrowserCompatibilityTester {
    fn default() -> Self {
        Self::new()
    }
}

impl BrowserCompatibilityTester {
    pub fn new() -> Self {
        Self {
            capability_matrix: BrowserCapabilityMatrix::default(),
        }
    }

    /// Test browser detection with various User-Agent strings
    pub fn test_browser_detection(&self) -> BrowserCompatibilityTestResults {
        let test_cases = vec![
            // Chrome
            (
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/91.0.4472.124 Safari/537.36",
                BrowserType::Chrome,
            ),
            // Firefox
            (
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:89.0) Gecko/20100101 Firefox/89.0",
                BrowserType::Firefox,
            ),
            // Safari
            (
                "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Version/14.1.1 Safari/537.36",
                BrowserType::Safari,
            ),
            // Edge
            (
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/91.0.4472.124 Safari/537.36 Edg/91.0.864.59",
                BrowserType::Edge,
            ),
            // Unknown
            ("Some Unknown Browser/1.0", BrowserType::Unknown),
        ];

        let mut correct_detections = 0;
        let total_tests = test_cases.len();
        let mut failed_cases = Vec::new();

        for (user_agent, expected_browser) in test_cases {
            let detected_browser = self
                .capability_matrix
                .detect_browser_type(&user_agent.to_lowercase());

            if detected_browser == expected_browser {
                correct_detections += 1;
            } else {
                failed_cases.push((user_agent.to_string(), expected_browser, detected_browser));
            }
        }

        BrowserCompatibilityTestResults {
            detection_accuracy: (correct_detections as f64 / total_tests as f64) * 100.0,
            total_tests,
            correct_detections,
            failed_cases,
        }
    }

    /// Test format support matrix
    pub fn test_format_support_matrix(&self) -> Vec<FormatSupportTest> {
        let browsers = vec![
            BrowserType::Chrome,
            BrowserType::Firefox,
            BrowserType::Safari,
            BrowserType::Edge,
            BrowserType::Unknown,
        ];

        let formats = vec!["video/mp4", "video/webm", "application/x-mpegURL"];

        let mut results = Vec::new();

        for browser in browsers {
            for format in &formats {
                let supports = self.capability_matrix.supports_mime_type(&browser, format);

                results.push(FormatSupportTest {
                    browser: browser.clone(),
                    format: format.to_string(),
                    supports,
                });
            }
        }

        results
    }

    /// Create test headers for specific browsers
    ///
    /// # Panics
    ///
    /// Panics if the user agent string cannot be parsed as a valid header value.
    pub fn create_test_headers(&self, browser_type: BrowserType) -> HeaderMap {
        let mut headers = HeaderMap::new();

        let user_agent = match browser_type {
            BrowserType::Chrome => {
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/91.0.4472.124 Safari/537.36"
            }
            BrowserType::Firefox => {
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:89.0) Gecko/20100101 Firefox/89.0"
            }
            BrowserType::Safari => {
                "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Version/14.1.1 Safari/537.36"
            }
            BrowserType::Edge => {
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/91.0.4472.124 Safari/537.36 Edg/91.0.864.59"
            }
            BrowserType::Unknown => "Unknown Browser/1.0",
        };

        headers.insert("user-agent", user_agent.parse().unwrap());
        headers
    }

    /// Test complete browser compatibility pipeline
    ///
    /// # Errors
    ///
    /// Returns `HttpStreamingError` if any of the compatibility tests fail
    /// or if there are issues with the browser detection matrix.
    pub fn run_browser_compatibility_tests(&self) -> Result<(), HttpStreamingError> {
        tracing::info!("Running browser compatibility test suite");

        // Test 1: Browser detection accuracy
        let detection_results = self.test_browser_detection();
        tracing::info!(
            "Browser detection: {:.1}% accuracy ({}/{} correct)",
            detection_results.detection_accuracy,
            detection_results.correct_detections,
            detection_results.total_tests
        );

        if detection_results.detection_accuracy < 100.0 {
            tracing::warn!("Browser detection failed for some cases:");
            for (user_agent, expected, actual) in &detection_results.failed_cases {
                tracing::warn!(
                    "  UA: {} | Expected: {:?} | Got: {:?}",
                    user_agent,
                    expected,
                    actual
                );
            }
        }

        // Test 2: Format support matrix
        let format_tests = self.test_format_support_matrix();
        tracing::info!("Format support matrix:");

        for test in &format_tests {
            let support_status = if test.supports { "✓" } else { "✗" };
            tracing::info!(
                "  {:?} + {} = {}",
                test.browser,
                test.format,
                support_status
            );
        }

        // Test 3: Header parsing for each browser type
        for browser in &[
            BrowserType::Chrome,
            BrowserType::Firefox,
            BrowserType::Safari,
            BrowserType::Edge,
        ] {
            let headers = self.create_test_headers(browser.clone());
            let capabilities = self.capability_matrix.detect_browser_capabilities(&headers);

            tracing::info!(
                "{:?}: MP4={} WebM={} HLS={}",
                browser,
                capabilities.supports_mp4,
                capabilities.supports_webm,
                capabilities.supports_hls
            );
        }

        // Success criteria: 100% browser detection accuracy
        if detection_results.detection_accuracy < 100.0 {
            return Err(HttpStreamingError::BrowserCompatibilityFailed {
                reason: format!(
                    "Browser detection accuracy {:.1}% < 100%",
                    detection_results.detection_accuracy
                ),
            });
        }

        tracing::info!("Browser compatibility tests passed");
        Ok(())
    }
}

/// Results of browser compatibility testing
#[derive(Debug)]
pub struct BrowserCompatibilityTestResults {
    pub detection_accuracy: f64,
    pub total_tests: usize,
    pub correct_detections: usize,
    pub failed_cases: Vec<(String, BrowserType, BrowserType)>,
}

/// Individual format support test result
#[derive(Debug)]
pub struct FormatSupportTest {
    pub browser: BrowserType,
    pub format: String,
    pub supports: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chrome_detection() {
        let tester = BrowserCompatibilityTester::new();
        let chrome_ua = "mozilla/5.0 (windows nt 10.0; win64; x64) applewebkit/537.36 (khtml, like gecko) chrome/91.0.4472.124 safari/537.36";

        let browser_type = tester.capability_matrix.detect_browser_type(chrome_ua);
        assert_eq!(browser_type, BrowserType::Chrome);
    }

    #[test]
    fn test_firefox_detection() {
        let tester = BrowserCompatibilityTester::new();
        let firefox_ua =
            "mozilla/5.0 (windows nt 10.0; win64; x64; rv:89.0) gecko/20100101 firefox/89.0";

        let browser_type = tester.capability_matrix.detect_browser_type(firefox_ua);
        assert_eq!(browser_type, BrowserType::Firefox);
    }

    #[test]
    fn test_safari_detection() {
        let tester = BrowserCompatibilityTester::new();
        let safari_ua = "mozilla/5.0 (macintosh; intel mac os x 10_15_7) applewebkit/537.36 (khtml, like gecko) version/14.1.1 safari/537.36";

        let browser_type = tester.capability_matrix.detect_browser_type(safari_ua);
        assert_eq!(browser_type, BrowserType::Safari);
    }

    #[test]
    fn test_edge_detection() {
        let tester = BrowserCompatibilityTester::new();
        let edge_ua = "mozilla/5.0 (windows nt 10.0; win64; x64) applewebkit/537.36 (khtml, like gecko) chrome/91.0.4472.124 safari/537.36 edg/91.0.864.59";

        let browser_type = tester.capability_matrix.detect_browser_type(edge_ua);
        assert_eq!(browser_type, BrowserType::Edge);
    }

    #[test]
    fn test_format_support_chrome() {
        let matrix = BrowserCapabilityMatrix::default();

        assert!(matrix.supports_mime_type(&BrowserType::Chrome, "video/mp4"));
        assert!(matrix.supports_mime_type(&BrowserType::Chrome, "video/webm"));
        assert!(matrix.supports_mime_type(&BrowserType::Chrome, "application/x-mpegURL"));
    }

    #[test]
    fn test_format_support_safari() {
        let matrix = BrowserCapabilityMatrix::default();

        assert!(matrix.supports_mime_type(&BrowserType::Safari, "video/mp4"));
        assert!(!matrix.supports_mime_type(&BrowserType::Safari, "video/webm"));
        assert!(matrix.supports_mime_type(&BrowserType::Safari, "application/x-mpegURL"));
    }

    #[test]
    fn test_browser_compatibility_suite() {
        let tester = BrowserCompatibilityTester::new();
        let results = tester.test_browser_detection();

        // Should achieve 100% detection accuracy
        assert_eq!(results.detection_accuracy, 100.0);
        assert_eq!(results.failed_cases.len(), 0);
    }

    #[test]
    fn test_header_parsing() {
        let tester = BrowserCompatibilityTester::new();
        let headers = tester.create_test_headers(BrowserType::Chrome);

        let capabilities = tester
            .capability_matrix
            .detect_browser_capabilities(&headers);

        assert!(capabilities.supports_mp4);
        assert!(capabilities.supports_webm);
        assert!(capabilities.supports_hls);
        assert!(capabilities.user_agent.contains("Chrome"));
    }
}

#!/usr/bin/env python3
"""
Automated HTTP streaming tests for Riptide server

This script tests the streaming functionality by making real HTTP requests
to a running Riptide server and validating the responses.

Usage:
    python test_streaming_http.py [--server-url http://127.0.0.1:3000]
"""

import argparse
import hashlib
import json
import os
import struct
import sys
import tempfile
import time
from typing import Dict, List, Optional, Tuple

import requests


class StreamingTester:
    """Test harness for Riptide streaming functionality"""

    def __init__(self, server_url: str = "http://127.0.0.1:3000"):
        self.server_url = server_url.rstrip("/")
        self.session = requests.Session()
        self.session.headers.update({
            "User-Agent": "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36"
        })
        self.test_results = []

    def log(self, level: str, message: str):
        """Log a message with timestamp"""
        timestamp = time.strftime("%Y-%m-%d %H:%M:%S")
        color = {
            "INFO": "\033[0m",      # Default
            "SUCCESS": "\033[92m",  # Green
            "WARNING": "\033[93m",  # Yellow
            "ERROR": "\033[91m",    # Red
            "DEBUG": "\033[94m",    # Blue
        }.get(level, "\033[0m")

        print(f"{color}[{timestamp}] [{level}] {message}\033[0m")

    def check_server_health(self) -> bool:
        """Check if the server is running and accessible"""
        try:
            response = self.session.get(f"{self.server_url}/", timeout=5)
            return response.status_code == 200
        except requests.exceptions.RequestException:
            return False

    def get_available_torrents(self) -> List[Dict]:
        """Get list of available torrents from the server"""
        try:
            response = self.session.get(f"{self.server_url}/torrents", timeout=10)
            if response.status_code == 200:
                # Parse HTML response to extract torrent info hashes
                import re
                html_content = response.text
                # Extract info hashes from HTML (40 character hex strings)
                info_hashes = re.findall(r'[0-9a-f]{40}', html_content)

                # Also try to get file names and formats
                torrents = []
                for hash_val in info_hashes:
                    # Try to extract additional info from the HTML
                    torrent_info = {"info_hash": hash_val}

                    # Look for file format indicators near the hash
                    hash_context = html_content[max(0, html_content.find(hash_val) - 200):
                                              html_content.find(hash_val) + 200]

                    if '.avi' in hash_context.lower():
                        torrent_info["format"] = "avi"
                    elif '.mkv' in hash_context.lower():
                        torrent_info["format"] = "mkv"
                    elif '.mp4' in hash_context.lower():
                        torrent_info["format"] = "mp4"

                    torrents.append(torrent_info)

                return torrents
        except Exception as e:
            self.log("ERROR", f"Failed to get torrents: {e}")
            return []

    def validate_mp4_structure(self, data: bytes) -> Tuple[bool, str]:
        """Validate MP4 file structure"""
        if len(data) < 12:
            return False, "File too small to be valid MP4"

        # Check for ftyp box
        if data[4:8] != b'ftyp':
            return False, f"Missing ftyp box, found: {data[4:8]}"

        # Scan for moov atom
        pos = 0
        found_moov = False
        found_mdat = False
        atoms = []

        while pos + 8 < len(data):
            try:
                size = struct.unpack('>I', data[pos:pos+4])[0]
                atom_type = data[pos+4:pos+8].decode('ascii', errors='ignore')
                atoms.append((atom_type, size, pos))

                if atom_type == 'moov':
                    found_moov = True
                elif atom_type == 'mdat':
                    found_mdat = True

                if size == 0 or pos + size > len(data):
                    break

                pos += size
            except:
                break

        if not found_moov:
            return False, f"Missing moov atom. Found atoms: {[a[0] for a in atoms[:10]]}"

        if not found_mdat:
            return False, f"Missing mdat atom. Found atoms: {[a[0] for a in atoms[:10]]}"

        # Check if moov is before mdat (faststart)
        moov_pos = next((a[2] for a in atoms if a[0] == 'moov'), -1)
        mdat_pos = next((a[2] for a in atoms if a[0] == 'mdat'), -1)

        if moov_pos > mdat_pos:
            self.log("WARNING", "moov atom after mdat - not optimized for streaming")

        return True, f"Valid MP4 with atoms: {[a[0] for a in atoms[:10]]}"

    def test_streaming_request(
        self,
        info_hash: str,
        range_header: Optional[str] = None,
        expected_status: int = 200
    ) -> Dict:
        """Test a streaming request"""
        headers = {}
        if range_header:
            headers["Range"] = range_header

        start_time = time.time()

        try:
            response = self.session.get(
                f"{self.server_url}/stream/{info_hash}",
                headers=headers,
                stream=True,
                timeout=30
            )

            elapsed = time.time() - start_time

            # Collect response data
            result = {
                "status_code": response.status_code,
                "headers": dict(response.headers),
                "elapsed": elapsed,
                "content_length": int(response.headers.get("Content-Length", 0)),
                "content_type": response.headers.get("Content-Type", ""),
                "success": response.status_code == expected_status
            }

            # Enhanced 503 error handling and debugging
            if response.status_code == 503:
                response_text = response.text
                result["response_text"] = response_text
                result["retry_after"] = response.headers.get("Retry-After", "unknown")

                # Log detailed 503 information
                self.log("DEBUG", f"503 Error Details for {info_hash}:")
                self.log("DEBUG", f"  Response: {response_text}")
                self.log("DEBUG", f"  Retry-After: {result['retry_after']}")
                self.log("DEBUG", f"  Content-Type: {result['content_type']}")
                self.log("DEBUG", f"  All Headers: {dict(response.headers)}")

            # For successful responses, validate content
            if response.status_code in [200, 206]:
                # Read first chunk to validate
                chunk_size = min(1024 * 1024, result["content_length"])  # Max 1MB
                content = b""
                for chunk in response.iter_content(chunk_size=8192):
                    content += chunk
                    if len(content) >= chunk_size:
                        break

                # Validate MP4 if content type indicates it
                if "mp4" in result["content_type"].lower():
                    is_valid, message = self.validate_mp4_structure(content)
                    result["mp4_valid"] = is_valid
                    result["mp4_validation_message"] = message
                else:
                    result["mp4_valid"] = None
                    result["mp4_validation_message"] = "Not an MP4 response"

            return result

        except Exception as e:
            return {
                "status_code": 0,
                "error": str(e),
                "elapsed": time.time() - start_time,
                "success": False
            }

    def test_avi_streaming(self, info_hash: str) -> bool:
        """Test AVI file streaming"""
        self.log("INFO", f"Testing AVI streaming for {info_hash}")

        # Test 1: Initial request (should trigger remux)
        result1 = self.test_streaming_request(info_hash)

        if result1["status_code"] == 0:
            self.log("ERROR", f"Connection failed: {result1.get('error')}")
            return False

        # Handle "Stream is being prepared" responses
        retry_count = 0
        while result1["status_code"] in [425, 503] and retry_count < 10:
            self.log("INFO", "Stream being prepared, waiting...")
            time.sleep(2)
            result1 = self.test_streaming_request(info_hash)
            retry_count += 1

        if result1["status_code"] != 200:
            self.log("ERROR", f"Unexpected status: {result1['status_code']}")
            return False

        if not result1.get("mp4_valid"):
            self.log("ERROR", f"Invalid MP4: {result1.get('mp4_validation_message')}")
            return False

        self.log("SUCCESS", f"AVI remuxed successfully in {result1['elapsed']:.2f}s")

        # Test 2: Cached request (should be faster)
        result2 = self.test_streaming_request(info_hash)

        if result2["elapsed"] < result1["elapsed"] * 0.5:
            self.log("SUCCESS", f"Cache working: {result2['elapsed']:.2f}s vs {result1['elapsed']:.2f}s")
        else:
            self.log("WARNING", "Cache may not be working properly")

        # Test 3: Range request
        range_result = self.test_streaming_request(
            info_hash,
            "bytes=1000-2000",
            expected_status=206
        )

        if range_result["status_code"] == 206:
            self.log("SUCCESS", "Range requests working")
        else:
            self.log("ERROR", f"Range request failed: {range_result['status_code']}")
            return False

        return True

    def test_mkv_streaming(self, info_hash: str) -> bool:
        """Test MKV file streaming with seeking"""
        self.log("INFO", f"Testing MKV streaming for {info_hash}")

        # Get file size first
        head_result = self.session.head(f"{self.server_url}/stream/{info_hash}")
        if head_result.status_code != 200:
            # Try with GET
            result = self.test_streaming_request(info_hash)
            file_size = result.get("content_length", 0)
        else:
            file_size = int(head_result.headers.get("Content-Length", 0))

        if file_size == 0:
            self.log("ERROR", "Could not determine file size")
            return False

        # Test multiple seek positions
        seek_positions = [
            0,
            file_size // 4,
            file_size // 2,
            file_size * 3 // 4
        ]

        all_success = True
        for pos in seek_positions:
            range_header = f"bytes={pos}-"
            result = self.test_streaming_request(
                info_hash,
                range_header,
                expected_status=206
            )

            if result["status_code"] == 206:
                content_range = result["headers"].get("Content-Range", "")
                if f"bytes {pos}-" in content_range:
                    self.log("SUCCESS", f"Seek to position {pos} successful")
                else:
                    self.log("ERROR", f"Invalid Content-Range: {content_range}")
                    all_success = False
            else:
                self.log("ERROR", f"Seek failed at position {pos}: {result['status_code']}")
                all_success = False

        return all_success

    def test_concurrent_requests(self, info_hash: str, num_requests: int = 5) -> bool:
        """Test concurrent requests to same file"""
        self.log("INFO", f"Testing {num_requests} concurrent requests")

        import concurrent.futures

        def make_request(i):
            return self.test_streaming_request(info_hash)

        with concurrent.futures.ThreadPoolExecutor(max_workers=num_requests) as executor:
            futures = [executor.submit(make_request, i) for i in range(num_requests)]
            results = [f.result() for f in concurrent.futures.as_completed(futures)]

        success_count = sum(1 for r in results if r.get("success"))
        preparing_count = sum(1 for r in results if r.get("status_code") in [425, 503])

        self.log("INFO", f"Results: {success_count} success, {preparing_count} preparing")

        return success_count > 0

    def test_cache_directory(self) -> bool:
        """Check cache directory status"""
        cache_dirs = [
            "/tmp/riptide-remux-cache",
            "/var/tmp/riptide-remux-cache",
            os.path.join(tempfile.gettempdir(), "riptide-remux-cache")
        ]

        for cache_dir in cache_dirs:
            if os.path.exists(cache_dir):
                self.log("INFO", f"Cache directory found: {cache_dir}")

                files = os.listdir(cache_dir)
                mp4_files = [f for f in files if f.endswith('.mp4')]
                lock_files = [f for f in files if f.endswith('.lock')]

                self.log("INFO", f"Cache contents: {len(mp4_files)} MP4 files, {len(lock_files)} lock files")

                # Validate cached MP4 files
                for mp4_file in mp4_files[:3]:  # Check first 3
                    path = os.path.join(cache_dir, mp4_file)
                    try:
                        with open(path, 'rb') as f:
                            data = f.read(1024 * 1024)  # Read first 1MB
                            is_valid, message = self.validate_mp4_structure(data)
                            if is_valid:
                                self.log("SUCCESS", f"Valid cached MP4: {mp4_file}")
                            else:
                                self.log("ERROR", f"Invalid cached MP4 {mp4_file}: {message}")
                    except Exception as e:
                        self.log("ERROR", f"Failed to validate {mp4_file}: {e}")

                return True

        self.log("WARNING", "No cache directory found")
        return False

    def run_all_tests(self, info_hashes: Optional[List[str]] = None) -> bool:
        """Run all streaming tests"""
        self.log("INFO", "Starting Riptide streaming tests")

        # Check server health
        if not self.check_server_health():
            self.log("ERROR", f"Server not responding at {self.server_url}")
            return False

        self.log("SUCCESS", "Server is running")

        # Get torrents if not provided
        if not info_hashes:
            torrents = self.get_available_torrents()
            if not torrents:
                self.log("WARNING", "No torrents found, using test hashes")
                # Use the hashes from your example
                info_hashes = [
                    "c1f839325f14bf485761059f23061b273d5c820f",  # AVI
                    "242d88012018d71594f073d03736ae99fc52f1c6"   # MKV
                ]
            else:
                info_hashes = [t["info_hash"] for t in torrents if "info_hash" in t]

        # Test each file
        all_passed = True
        for info_hash in info_hashes:
            self.log("INFO", f"\nTesting torrent: {info_hash}")

            # Test basic streaming functionality
            passed = self.test_basic_streaming(info_hash)

            if not passed:
                all_passed = False

            # Test 503 error handling specifically
            if not self.test_503_error_handling(info_hash):
                self.log("WARNING", "503 error handling test had issues")

            # Test concurrent requests
            if not self.test_concurrent_requests(info_hash):
                self.log("WARNING", "Concurrent request test had issues")

        # Check cache
        self.test_cache_directory()

        return all_passed

    def test_basic_streaming(self, info_hash: str) -> bool:
        """Test basic streaming functionality"""
        self.log("INFO", f"Testing basic streaming for {info_hash}")

        # Test 1: Initial request
        result = self.test_streaming_request(info_hash)

        if result["status_code"] == 0:
            self.log("ERROR", f"Connection failed: {result.get('error')}")
            return False

        # Handle "Stream is being prepared" responses
        retry_count = 0
        consecutive_503s = 0
        while result["status_code"] in [425, 503] and retry_count < 10:
            if result["status_code"] == 503:
                consecutive_503s += 1
                self.log("WARNING", f"503 error #{consecutive_503s}: {result.get('response_text', 'No response text')}")

                # If we get too many 503s, this indicates a persistent issue
                if consecutive_503s >= 5:
                    self.log("ERROR", "Too many consecutive 503 errors - possible streaming issue")
                    return False

            self.log("INFO", f"Stream being prepared (attempt {retry_count + 1}/10), waiting...")
            time.sleep(2)
            result = self.test_streaming_request(info_hash)
            retry_count += 1

        if result["status_code"] not in [200, 206]:
            self.log("ERROR", f"Unexpected status after retries: {result['status_code']}")
            if result["status_code"] == 503:
                self.log("ERROR", f"Final 503 error: {result.get('response_text', 'No response text')}")
            return False

        # Check if response is valid MP4 if content type indicates it
        if result.get("mp4_valid") is False:
            self.log("ERROR", f"Invalid MP4: {result.get('mp4_validation_message')}")
            return False

        self.log("SUCCESS", f"Basic streaming test passed in {result['elapsed']:.2f}s")
        return True

    def test_503_error_handling(self, info_hash: str) -> bool:
        """Test 503 error handling and debugging"""
        self.log("INFO", f"Testing 503 error handling for {info_hash}")

        # Make multiple rapid requests to potentially trigger 503s
        error_503_count = 0
        total_requests = 5

        for i in range(total_requests):
            result = self.test_streaming_request(info_hash)

            if result["status_code"] == 503:
                error_503_count += 1
                self.log("DEBUG", f"Request {i+1}: Got 503 error")
                self.log("DEBUG", f"  Response: {result.get('response_text', 'No response')}")
                self.log("DEBUG", f"  Retry-After: {result.get('retry_after', 'Not specified')}")

                # Test that retry-after header is present
                if "retry_after" not in result or result["retry_after"] == "unknown":
                    self.log("WARNING", "503 response missing Retry-After header")

                # Test that content type is appropriate
                if result.get("content_type") != "video/mp4":
                    self.log("WARNING", f"503 response has unexpected content type: {result.get('content_type')}")

            elif result["status_code"] == 200:
                self.log("DEBUG", f"Request {i+1}: Got 200 response")
            else:
                self.log("DEBUG", f"Request {i+1}: Got {result['status_code']} response")

            time.sleep(0.5)  # Small delay between requests

        if error_503_count > 0:
            self.log("WARNING", f"Found {error_503_count}/{total_requests} 503 errors")
            self.log("INFO", "This indicates remux streaming issues that need investigation")

            # Additional debugging: check if this is a remux-required file
            self.log("DEBUG", "Checking if this file requires remuxing...")

            # Try to get file info to understand format
            try:
                response = self.session.head(f"{self.server_url}/stream/{info_hash}")
                if response.status_code == 200:
                    content_type = response.headers.get("Content-Type", "")
                    self.log("DEBUG", f"File content type: {content_type}")

                    if "mp4" not in content_type.lower():
                        self.log("INFO", "File appears to need remuxing (non-MP4 format)")
                else:
                    self.log("DEBUG", f"HEAD request failed with {response.status_code}")
            except Exception as e:
                self.log("DEBUG", f"HEAD request error: {e}")

        return True  # We expect 503s in some cases, so don't fail the test


def main():
    parser = argparse.ArgumentParser(description="Test Riptide streaming functionality")
    parser.add_argument(
        "--server-url",
        default="http://127.0.0.1:3000",
        help="Riptide server URL"
    )
    parser.add_argument(
        "--info-hash",
        action="append",
        help="Specific info hash to test (can specify multiple)"
    )
    parser.add_argument(
        "--verbose",
        action="store_true",
        help="Enable verbose output"
    )
    parser.add_argument(
        "--detect-503",
        action="store_true",
        help="Focus on detecting and debugging 503 errors"
    )

    args = parser.parse_args()

    tester = StreamingTester(args.server_url)

    try:
        if args.detect_503:
            tester.log("INFO", "Running in 503 error detection mode")
            # Get all available torrents and test each one specifically for 503s
            torrents = tester.get_available_torrents()
            if not torrents:
                tester.log("ERROR", "No torrents available for 503 testing")
                sys.exit(1)

            found_503_errors = False
            for torrent in torrents:
                info_hash = torrent["info_hash"]
                tester.log("INFO", f"Testing {info_hash} for 503 errors...")

                # Test this specific hash multiple times
                for attempt in range(3):
                    result = tester.test_streaming_request(info_hash)
                    if result["status_code"] == 503:
                        found_503_errors = True
                        tester.log("ERROR", f"Found 503 error on attempt {attempt + 1}")
                        tester.log("ERROR", f"Response: {result.get('response_text', 'No response')}")
                        break
                    elif result["status_code"] == 200:
                        tester.log("SUCCESS", f"Request successful on attempt {attempt + 1}")
                        break
                    else:
                        tester.log("INFO", f"Got status {result['status_code']} on attempt {attempt + 1}")

                    time.sleep(1)

            if found_503_errors:
                tester.log("ERROR", "503 errors detected! Check server logs for remux issues.")
                sys.exit(1)
            else:
                tester.log("SUCCESS", "No 503 errors found in testing")
                sys.exit(0)
        else:
            success = tester.run_all_tests(args.info_hash)

            if success:
                tester.log("SUCCESS", "All tests passed! ✅")
                sys.exit(0)
            else:
                tester.log("ERROR", "Some tests failed! ❌")
                sys.exit(1)

    except KeyboardInterrupt:
        tester.log("INFO", "Tests interrupted by user")
        sys.exit(1)
    except Exception as e:
        tester.log("ERROR", f"Unexpected error: {e}")
        sys.exit(1)


if __name__ == "__main__":
    main()

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
        }.get(level, "\033[0m")

        print(f"{color}[{timestamp}] [{level}] {message}\033[0m")

    def check_server_health(self) -> bool:
        """Check if the server is running and accessible"""
        try:
            response = self.session.get(f"{self.server_url}/health", timeout=5)
            return response.status_code in [200, 404]  # 404 if no health endpoint
        except requests.exceptions.RequestException:
            return False

    def get_available_torrents(self) -> List[Dict]:
        """Get list of available torrents from the server"""
        try:
            response = self.session.get(f"{self.server_url}/api/torrents", timeout=10)
            if response.status_code == 200:
                return response.json()
            else:
                # Try alternate endpoint
                response = self.session.get(f"{self.server_url}/torrents", timeout=10)
                if response.status_code == 200:
                    # Parse HTML response to extract torrent info
                    # This is a simplified version - adjust based on actual HTML structure
                    return []
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
                info_hashes = [t.get("info_hash") for t in torrents]

        # Test each file
        all_passed = True
        for info_hash in info_hashes:
            self.log("INFO", f"\nTesting torrent: {info_hash}")

            # Determine file type (simplified - would need actual detection)
            if info_hash.endswith("820f"):  # AVI hash pattern
                passed = self.test_avi_streaming(info_hash)
            else:  # Assume MKV
                passed = self.test_mkv_streaming(info_hash)

            if not passed:
                all_passed = False

            # Test concurrent requests
            if not self.test_concurrent_requests(info_hash):
                self.log("WARNING", "Concurrent request test had issues")

        # Check cache
        self.test_cache_directory()

        return all_passed


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

    args = parser.parse_args()

    tester = StreamingTester(args.server_url)

    try:
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

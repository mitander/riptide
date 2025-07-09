#!/usr/bin/env python3
"""
Real-world streaming debug test for Riptide

This script creates actual video files, starts the server, and tests
streaming with real browser behavior to identify actual issues.
"""

import os
import re
import subprocess
import sys
import time
import tempfile
import shutil
from typing import Dict, List, Optional, Tuple
import requests
import struct

class RiptideStreamingDebugger:
    def __init__(self, server_port: int = 3000):
        self.server_port = server_port
        self.server_url = f"http://127.0.0.1:{server_port}"
        self.temp_dir = None
        self.server_process = None
        self.test_files = {}

    def log(self, level: str, message: str):
        """Log with timestamp and color"""
        colors = {
            "INFO": "\033[36m",     # Cyan
            "SUCCESS": "\033[92m",  # Green
            "WARNING": "\033[93m",  # Yellow
            "ERROR": "\033[91m",    # Red
            "DEBUG": "\033[90m",    # Gray
        }
        color = colors.get(level, "\033[0m")
        print(f"{color}[{level}] {message}\033[0m")

    def create_test_videos(self) -> bool:
        """Create real test video files with different formats"""
        self.log("INFO", "Creating real test video files...")

        try:
            # Create problematic AVI file (MP3 audio, MPEG4 video)
            avi_path = os.path.join(self.temp_dir, "problem.avi")
            cmd = [
                "ffmpeg", "-f", "lavfi", "-i", "testsrc2=duration=10:size=640x480:rate=25",
                "-f", "lavfi", "-i", "sine=frequency=1000:duration=10",
                "-c:v", "mpeg4", "-c:a", "mp3", "-b:v", "2M", "-b:a", "128k",
                "-y", avi_path
            ]
            result = subprocess.run(cmd, capture_output=True, text=True)
            if result.returncode != 0:
                self.log("ERROR", f"Failed to create AVI: {result.stderr}")
                return False
            self.test_files["avi"] = avi_path
            self.log("SUCCESS", f"Created AVI: {os.path.getsize(avi_path)} bytes")

            # Create MKV file with H.264/AAC
            mkv_path = os.path.join(self.temp_dir, "test.mkv")
            cmd = [
                "ffmpeg", "-f", "lavfi", "-i", "testsrc2=duration=15:size=1280x720:rate=30",
                "-f", "lavfi", "-i", "sine=frequency=800:duration=15",
                "-c:v", "libx264", "-preset", "fast", "-c:a", "aac",
                "-b:v", "3M", "-b:a", "192k", "-y", mkv_path
            ]
            result = subprocess.run(cmd, capture_output=True, text=True)
            if result.returncode != 0:
                self.log("ERROR", f"Failed to create MKV: {result.stderr}")
                return False
            self.test_files["mkv"] = mkv_path
            self.log("SUCCESS", f"Created MKV: {os.path.getsize(mkv_path)} bytes")

            # Create properly formatted MP4 for comparison
            mp4_path = os.path.join(self.temp_dir, "reference.mp4")
            cmd = [
                "ffmpeg", "-f", "lavfi", "-i", "testsrc2=duration=10:size=640x480:rate=25",
                "-f", "lavfi", "-i", "sine=frequency=600:duration=10",
                "-c:v", "libx264", "-preset", "fast", "-c:a", "aac",
                "-b:v", "2M", "-b:a", "128k", "-movflags", "+faststart",
                "-y", mp4_path
            ]
            result = subprocess.run(cmd, capture_output=True, text=True)
            if result.returncode != 0:
                self.log("ERROR", f"Failed to create MP4: {result.stderr}")
                return False
            self.test_files["mp4"] = mp4_path
            self.log("SUCCESS", f"Created MP4: {os.path.getsize(mp4_path)} bytes")

            return True

        except Exception as e:
            self.log("ERROR", f"Exception creating videos: {e}")
            return False

    def analyze_video_file(self, file_path: str, format_name: str):
        """Analyze video file properties"""
        self.log("DEBUG", f"Analyzing {format_name} file...")

        # Get format info
        cmd = ["ffprobe", "-v", "quiet", "-show_format", "-show_streams", file_path]
        result = subprocess.run(cmd, capture_output=True, text=True)

        if result.returncode == 0:
            output = result.stdout
            # Extract key info
            video_codec = re.search(r'codec_name=(\w+)', output)
            audio_codec = re.search(r'codec_name=(\w+).*codec_type=audio', output, re.DOTALL)
            format_name_probe = re.search(r'format_name=([^\n]+)', output)
            duration = re.search(r'duration=([0-9.]+)', output)

            self.log("INFO", f"{format_name} analysis:")
            self.log("INFO", f"  Video codec: {video_codec.group(1) if video_codec else 'unknown'}")
            self.log("INFO", f"  Audio codec: {audio_codec.group(1) if audio_codec else 'unknown'}")
            self.log("INFO", f"  Format: {format_name_probe.group(1) if format_name_probe else 'unknown'}")
            self.log("INFO", f"  Duration: {duration.group(1) if duration else 'unknown'}s")

    def start_server(self) -> bool:
        """Start Riptide server"""
        self.log("INFO", "Starting Riptide server...")

        try:
            cmd = [
                "./target/release/riptide", "server",
                "--mode", "development",
                "--port", str(self.server_port),
                "--movies-dir", self.temp_dir
            ]

            log_file = os.path.join(self.temp_dir, "server.log")
            with open(log_file, 'w') as f:
                self.server_process = subprocess.Popen(
                    cmd, stdout=f, stderr=subprocess.STDOUT,
                    env=dict(os.environ, RUST_LOG="info")
                )

            # Wait for server to start
            for attempt in range(30):
                try:
                    response = requests.get(f"{self.server_url}/", timeout=2)
                    if response.status_code == 200:
                        self.log("SUCCESS", "Server started successfully")
                        time.sleep(2)  # Give it time to process files
                        return True
                except:
                    pass
                time.sleep(1)

            self.log("ERROR", "Server failed to start within 30 seconds")
            return False

        except Exception as e:
            self.log("ERROR", f"Failed to start server: {e}")
            return False

    def get_torrent_hashes(self) -> List[str]:
        """Extract torrent info hashes from server and logs"""
        hashes = []

        # Try to get from web page first
        try:
            response = requests.get(f"{self.server_url}/torrents", timeout=10)
            if response.status_code == 200:
                # Extract 40-character hex strings (info hashes)
                page_hashes = re.findall(r'[0-9a-f]{40}', response.text)
                hashes.extend(page_hashes)
        except Exception as e:
            self.log("WARNING", f"Could not get hashes from web page: {e}")

        # Extract from server logs as backup
        log_hashes = self.extract_hashes_from_logs()
        hashes.extend(log_hashes)

        unique_hashes = list(set(hashes))
        self.log("INFO", f"Found {len(unique_hashes)} unique torrent hashes")
        return unique_hashes

    def inspect_server_logs(self) -> None:
        """Inspect server logs to understand what's happening"""
        log_file = os.path.join(self.temp_dir, "server.log")
        if not os.path.exists(log_file):
            self.log("ERROR", "Server log file not found")
            return

        self.log("INFO", "Inspecting server logs...")
        try:
            with open(log_file, 'r') as f:
                logs = f.read()

            # Look for key events
            if "Found" in logs and "movie" in logs:
                movies_found = re.findall(r'Found (\d+) movie files', logs)
                if movies_found:
                    self.log("INFO", f"  Server found {movies_found[0]} movie files")

            if "Converting" in logs:
                conversions = re.findall(r'Converting (\w+) to BitTorrent', logs)
                self.log("INFO", f"  Converting files: {conversions}")

            if "✓ Converted" in logs:
                converted = re.findall(r'✓ Converted (\w+) \((\d+) pieces', logs)
                for name, pieces in converted:
                    self.log("SUCCESS", f"  Successfully converted {name} ({pieces} pieces)")

            if "Background conversions completed" in logs:
                completed = re.search(r'Background conversions completed: (\d+) successful, (\d+) failed', logs)
                if completed:
                    self.log("INFO", f"  Conversions: {completed.group(1)} successful, {completed.group(2)} failed")

            # Look for errors
            error_lines = [line for line in logs.split('\n') if 'ERROR' in line or 'error' in line.lower()]
            if error_lines:
                self.log("WARNING", "  Error messages found:")
                for error in error_lines[:5]:  # Show first 5 errors
                    self.log("WARNING", f"    {error.strip()}")

            # Show last few lines for context
            last_lines = logs.strip().split('\n')[-10:]
            self.log("DEBUG", "  Last 10 log lines:")
            for line in last_lines:
                if line.strip():
                    self.log("DEBUG", f"    {line}")

        except Exception as e:
            self.log("ERROR", f"Failed to read server logs: {e}")

    def extract_hashes_from_logs(self) -> List[str]:
        """Extract info hashes from server logs"""
        log_file = os.path.join(self.temp_dir, "server.log")
        if not os.path.exists(log_file):
            return []

        try:
            with open(log_file, 'r') as f:
                logs = f.read()

            # Look for torrent hashes in log messages
            hashes = re.findall(r'torrent [0-9a-f]{40}', logs)
            # Clean up the matches to just get the hash part
            clean_hashes = [h.replace('torrent ', '') for h in hashes]
            return clean_hashes

        except Exception as e:
            self.log("ERROR", f"Failed to extract hashes from logs: {e}")
            return []

    def wait_for_downloads_complete(self, timeout: int = 60) -> bool:
        """Wait for torrent downloads to complete"""
        self.log("INFO", "Waiting for downloads to complete...")

        start_time = time.time()
        while time.time() - start_time < timeout:
            log_file = os.path.join(self.temp_dir, "server.log")
            if os.path.exists(log_file):
                try:
                    with open(log_file, 'r') as f:
                        logs = f.read()

                    # Look for completion messages
                    completed_downloads = re.findall(r'Download completed successfully for torrent', logs)
                    conversions_completed = 'Background conversions completed' in logs

                    if conversions_completed and len(completed_downloads) >= 3:
                        self.log("SUCCESS", "All downloads completed")
                        return True

                except Exception:
                    pass

            time.sleep(2)

        self.log("WARNING", f"Timeout waiting for downloads to complete ({timeout}s)")
        return False

    def validate_mp4_structure(self, data: bytes, name: str) -> Tuple[bool, str]:
        """Validate MP4 file structure in detail"""
        if len(data) < 12:
            return False, f"{name}: File too small ({len(data)} bytes)"

        # Check ftyp box
        if data[4:8] != b'ftyp':
            return False, f"{name}: Missing ftyp box, found: {data[4:8]}"

        # Parse atoms
        pos = 0
        atoms = []
        found_moov = False
        found_mdat = False
        moov_pos = -1
        mdat_pos = -1

        while pos + 8 < len(data):
            try:
                size = struct.unpack('>I', data[pos:pos+4])[0]
                atom_type = data[pos+4:pos+8]

                try:
                    atom_name = atom_type.decode('ascii')
                except:
                    atom_name = str(atom_type)

                atoms.append((atom_name, size, pos))

                if atom_name == 'moov':
                    found_moov = True
                    moov_pos = pos
                elif atom_name == 'mdat':
                    found_mdat = True
                    mdat_pos = pos

                if size == 0 or size > len(data) - pos:
                    break

                pos += size
            except:
                break

        issues = []
        if not found_moov:
            issues.append("Missing moov atom")
        if not found_mdat:
            issues.append("Missing mdat atom")
        if found_moov and found_mdat and moov_pos > mdat_pos:
            issues.append("moov after mdat (not faststart optimized)")

        atom_names = [a[0] for a in atoms[:10]]

        if issues:
            return False, f"{name}: {'; '.join(issues)}. Atoms: {atom_names}"
        else:
            return True, f"{name}: Valid MP4. Atoms: {atom_names}"

    def test_streaming_endpoint(self, info_hash: str, test_name: str) -> Dict:
        """Test streaming endpoint with browser-like requests"""
        self.log("INFO", f"Testing streaming for {test_name} ({info_hash[:8]}...)")

        results = {}
        session = requests.Session()
        session.headers.update({
            'User-Agent': 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36'
        })

        # Test 1: Initial HEAD request (browser pre-flight)
        try:
            start_time = time.time()
            head_response = session.head(f"{self.server_url}/stream/{info_hash}", timeout=30)
            head_time = time.time() - start_time

            results['head'] = {
                'status': head_response.status_code,
                'headers': dict(head_response.headers),
                'time': head_time
            }
            self.log("INFO", f"  HEAD: {head_response.status_code} in {head_time:.2f}s")

            if 'Content-Type' in head_response.headers:
                content_type = head_response.headers['Content-Type']
                self.log("INFO", f"  Content-Type: {content_type}")
                if content_type != 'video/mp4':
                    self.log("WARNING", f"  Expected video/mp4, got {content_type}")

        except Exception as e:
            results['head'] = {'error': str(e)}
            self.log("ERROR", f"  HEAD request failed: {e}")

        # Test 2: Full GET request
        try:
            start_time = time.time()
            get_response = session.get(f"{self.server_url}/stream/{info_hash}", timeout=60)
            get_time = time.time() - start_time

            results['get'] = {
                'status': get_response.status_code,
                'headers': dict(get_response.headers),
                'content_length': len(get_response.content),
                'time': get_time
            }

            self.log("INFO", f"  GET: {get_response.status_code} in {get_time:.2f}s")
            self.log("INFO", f"  Response size: {len(get_response.content)} bytes")

            if get_response.status_code == 200:
                # Validate MP4 structure
                is_valid, message = self.validate_mp4_structure(get_response.content, test_name)
                results['mp4_validation'] = {'valid': is_valid, 'message': message}

                if is_valid:
                    self.log("SUCCESS", f"  {message}")
                else:
                    self.log("ERROR", f"  {message}")

                # Check Content-Type consistency
                content_type = get_response.headers.get('Content-Type', '')
                if content_type != 'video/mp4':
                    self.log("WARNING", f"  GET Content-Type: {content_type} (expected video/mp4)")

        except Exception as e:
            results['get'] = {'error': str(e)}
            self.log("ERROR", f"  GET request failed: {e}")

        # Test 3: Range request (seeking)
        try:
            range_header = 'bytes=1000-5000'
            start_time = time.time()
            range_response = session.get(
                f"{self.server_url}/stream/{info_hash}",
                headers={'Range': range_header},
                timeout=30
            )
            range_time = time.time() - start_time

            results['range'] = {
                'status': range_response.status_code,
                'headers': dict(range_response.headers),
                'content_length': len(range_response.content),
                'time': range_time
            }

            self.log("INFO", f"  RANGE: {range_response.status_code} in {range_time:.2f}s")

            if range_response.status_code == 206:
                content_range = range_response.headers.get('Content-Range', '')
                self.log("SUCCESS", f"  Content-Range: {content_range}")
            else:
                self.log("WARNING", f"  Expected 206, got {range_response.status_code}")

        except Exception as e:
            results['range'] = {'error': str(e)}
            self.log("ERROR", f"  Range request failed: {e}")

        return results

    def test_mime_type_flashing(self, info_hash: str, test_name: str):
        """Test for MIME type flashing issue"""
        self.log("INFO", f"Testing MIME type consistency for {test_name}...")

        content_types = []

        # Make multiple rapid requests to catch flashing
        for i in range(5):
            try:
                response = requests.head(f"{self.server_url}/stream/{info_hash}", timeout=10)
                ct = response.headers.get('Content-Type', 'missing')
                content_types.append(ct)
                time.sleep(0.1)  # Small delay
            except Exception as e:
                content_types.append(f"error: {e}")

        unique_types = set(content_types)
        if len(unique_types) > 1:
            self.log("ERROR", f"  MIME type flashing detected! Types seen: {unique_types}")
            for i, ct in enumerate(content_types):
                self.log("ERROR", f"    Request {i+1}: {ct}")
        else:
            self.log("SUCCESS", f"  MIME type consistent: {unique_types.pop()}")

    def test_cache_behavior(self, info_hash: str, test_name: str):
        """Test caching behavior"""
        self.log("INFO", f"Testing cache behavior for {test_name}...")

        # First request (should be slow - remuxing)
        start_time = time.time()
        try:
            response1 = requests.get(f"{self.server_url}/stream/{info_hash}", timeout=60)
            first_time = time.time() - start_time

            # Second request (should be fast - cached)
            start_time = time.time()
            response2 = requests.get(f"{self.server_url}/stream/{info_hash}", timeout=30)
            second_time = time.time() - start_time

            self.log("INFO", f"  First request: {first_time:.2f}s")
            self.log("INFO", f"  Second request: {second_time:.2f}s")

            if second_time < first_time * 0.5:
                self.log("SUCCESS", f"  Cache working: {second_time:.2f}s vs {first_time:.2f}s")
            else:
                self.log("WARNING", f"  Cache may not be working optimally")

            # Check if responses are identical
            if response1.content == response2.content:
                self.log("SUCCESS", "  Response content identical")
            else:
                self.log("ERROR", "  Response content differs between requests!")

        except Exception as e:
            self.log("ERROR", f"  Cache test failed: {e}")

    def cleanup(self):
        """Clean up resources"""
        if self.server_process:
            self.log("INFO", "Stopping server...")
            self.server_process.terminate()
            try:
                self.server_process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self.server_process.kill()

        if self.temp_dir and os.path.exists(self.temp_dir):
            self.log("INFO", "Cleaning up temp files...")
            shutil.rmtree(self.temp_dir)

    def run_debug_session(self):
        """Run complete debugging session"""
        self.log("INFO", "Starting Riptide streaming debug session...")

        try:
            # Create temp directory
            self.temp_dir = tempfile.mkdtemp(prefix="riptide_debug_")
            self.log("INFO", f"Using temp directory: {self.temp_dir}")

            # Create test videos
            if not self.create_test_videos():
                return False

            # Analyze video files
            for format_name, file_path in self.test_files.items():
                self.analyze_video_file(file_path, format_name)

            # Start server
            if not self.start_server():
                return False

            # Wait for downloads to complete
            if not self.wait_for_downloads_complete():
                self.log("WARNING", "Downloads may not be complete, continuing anyway...")

            # Get torrent hashes
            hashes = self.get_torrent_hashes()
            if not hashes:
                self.log("ERROR", "No torrents found")
                self.inspect_server_logs()
                return False

            # Test each torrent
            for i, info_hash in enumerate(hashes[:3]):  # Test first 3
                format_guess = ["avi", "mkv", "mp4"][i] if i < 3 else "unknown"
                self.log("INFO", f"\n=== Testing {format_guess} torrent {info_hash} ===")

                # Test basic streaming
                results = self.test_streaming_endpoint(info_hash, format_guess)

                # Test MIME type flashing
                self.test_mime_type_flashing(info_hash, format_guess)

                # Test cache behavior
                self.test_cache_behavior(info_hash, format_guess)

                print()  # Spacing

            self.log("SUCCESS", "Debug session completed!")
            return True

        except Exception as e:
            self.log("ERROR", f"Debug session failed: {e}")
            return False
        finally:
            self.cleanup()

def main():
    if len(sys.argv) > 1 and sys.argv[1] in ['-h', '--help']:
        print("Usage: python3 debug_streaming.py [port]")
        print("Debug real-world streaming issues in Riptide")
        return

    port = int(sys.argv[1]) if len(sys.argv) > 1 else 3000

    debugger = RiptideStreamingDebugger(port)
    success = debugger.run_debug_session()

    sys.exit(0 if success else 1)

if __name__ == "__main__":
    main()

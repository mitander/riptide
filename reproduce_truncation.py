#!/usr/bin/env python3
"""
Automated reproduction script for progressive streaming truncation issue.

This script replicates the exact browser behavior to identify where and why
progressive streaming stops serving the full movie length.
"""

import time
import requests
import subprocess
import threading
import sys
import json
from datetime import datetime, timedelta

class ProgressiveStreamingTest:
    def __init__(self):
        self.base_url = "http://127.0.0.1:3000"
        self.server_process = None
        self.results = []
        
    def start_server(self):
        """Start the Riptide server in development mode"""
        print("🚀 Starting Riptide server...")
        self.server_process = subprocess.Popen(
            ["cargo", "run"],
            cwd="/Users/mitander/c/p/riptide",
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
            bufsize=1,
            universal_newlines=True
        )
        
        # Monitor server startup in background
        def monitor_server():
            for line in iter(self.server_process.stdout.readline, ''):
                if "Riptide media server running on" in line:
                    print("✅ Server started successfully")
                    break
                elif "error" in line.lower():
                    print(f"❌ Server error: {line.strip()}")
        
        monitor_thread = threading.Thread(target=monitor_server, daemon=True)
        monitor_thread.start()
        
        # Wait for server to be ready
        for i in range(30):
            try:
                response = requests.get(f"{self.base_url}/api/torrents", timeout=1)
                if response.status_code == 200:
                    print(f"✅ Server is responding (attempt {i+1})")
                    time.sleep(2)  # Additional stabilization time
                    return True
            except:
                time.sleep(1)
        
        print("❌ Server failed to start within 30 seconds")
        return False
    
    def get_torrents(self):
        """Get the list of available torrents"""
        try:
            response = requests.get(f"{self.base_url}/api/torrents")
            response.raise_for_status()
            
            data = response.json()
            
            # Handle both direct array and nested torrents format
            if isinstance(data, list):
                torrents = data
            elif isinstance(data, dict) and 'torrents' in data:
                torrents = data['torrents']
            else:
                print(f"❌ Unexpected API response format: {data}")
                return None
                
            if not torrents:
                print("❌ No torrents found")
                return None
                
            torrent = torrents[0]
            print(f"📁 Found torrent: {torrent.get('name', 'Unknown')}")
            print(f"🔗 Info hash: {torrent['info_hash']}")
            print(f"📊 Progress: {torrent.get('progress', 0):.1f}%")
            
            return torrent
            
        except Exception as e:
            print(f"❌ Failed to get torrents: {e}")
            return None
    
    def initial_stream_request(self, info_hash):
        """Make the initial streaming request (HEAD + GET with range=bytes=0-)"""
        stream_url = f"{self.base_url}/stream/{info_hash}"
        
        try:
            # Step 1: HEAD request (like browsers do)
            print("📡 Making HEAD request...")
            head_response = requests.head(stream_url)
            print(f"   Status: {head_response.status_code}")
            print(f"   Content-Length: {head_response.headers.get('Content-Length', 'Not set')}")
            print(f"   Accept-Ranges: {head_response.headers.get('Accept-Ranges', 'Not set')}")
            
            # Step 2: Initial range request (bytes=0-)
            print("📡 Making initial range request (bytes=0-)...")
            headers = {"Range": "bytes=0-"}
            response = requests.get(stream_url, headers=headers, stream=True)
            
            print(f"   Status: {response.status_code}")
            print(f"   Content-Range: {response.headers.get('Content-Range', 'Not set')}")
            print(f"   Content-Length: {response.headers.get('Content-Length', 'Not set')}")
            print(f"   Content-Type: {response.headers.get('Content-Type', 'Not set')}")
            
            # Read just the first chunk to trigger progressive streaming
            chunk = next(response.iter_content(chunk_size=8192), None)
            if chunk:
                print(f"   📦 Received first chunk: {len(chunk)} bytes")
                return response.headers.get('Content-Range'), len(chunk)
            
        except Exception as e:
            print(f"❌ Initial stream request failed: {e}")
            return None, 0
    
    def monitor_progressive_growth(self, info_hash, duration_minutes=5):
        """Monitor how the progressive stream grows over time"""
        stream_url = f"{self.base_url}/stream/{info_hash}"
        start_time = time.time()
        end_time = start_time + (duration_minutes * 60)
        
        print(f"⏱️  Monitoring progressive streaming for {duration_minutes} minutes...")
        print("📈 Tracking stream size growth:")
        
        last_size = 0
        consecutive_no_growth = 0
        max_consecutive_no_growth = 10  # Stop if no growth for 10 checks
        
        check_interval = 5  # Check every 5 seconds
        
        while time.time() < end_time:
            try:
                # Make a range request from where we left off
                headers = {"Range": f"bytes={last_size}-"}
                response = requests.head(stream_url, headers=headers)
                
                content_range = response.headers.get('Content-Range', '')
                
                # Parse content range: "bytes start-end/total" or "bytes start-end/*"
                current_size = last_size
                if content_range.startswith('bytes '):
                    parts = content_range[6:].split('/')
                    if len(parts) == 2:
                        range_part = parts[0]
                        total_part = parts[1]
                        
                        if '-' in range_part:
                            try:
                                end_byte = int(range_part.split('-')[1])
                                current_size = end_byte + 1
                            except:
                                pass
                
                elapsed = time.time() - start_time
                elapsed_str = f"{elapsed:.0f}s"
                
                if current_size > last_size:
                    growth = current_size - last_size
                    print(f"   {elapsed_str:>4}: {current_size:>10,} bytes (+{growth:,})")
                    last_size = current_size
                    consecutive_no_growth = 0
                    
                    # Record this data point
                    self.results.append({
                        'timestamp': time.time(),
                        'elapsed_seconds': elapsed,
                        'stream_size_bytes': current_size,
                        'growth_bytes': growth if 'growth' in locals() else 0
                    })
                else:
                    consecutive_no_growth += 1
                    if consecutive_no_growth <= 3:  # Only show first few no-growth messages
                        print(f"   {elapsed_str:>4}: {current_size:>10,} bytes (no growth)")
                    elif consecutive_no_growth == 4:
                        print(f"   ...continuing to monitor...")
                    
                    if consecutive_no_growth >= max_consecutive_no_growth:
                        print(f"🛑 Stopping: No growth detected for {max_consecutive_no_growth * check_interval} seconds")
                        break
                
                time.sleep(check_interval)
                
            except Exception as e:
                print(f"❌ Error during monitoring: {e}")
                time.sleep(check_interval)
        
        return last_size
    
    def analyze_results(self, original_file_size):
        """Analyze the progressive streaming results"""
        if not self.results:
            print("❌ No results to analyze")
            return
        
        final_size = self.results[-1]['stream_size_bytes']
        percentage_served = (final_size / original_file_size) * 100
        
        print("\n📊 ANALYSIS RESULTS:")
        print("=" * 50)
        print(f"Original file size: {original_file_size:,} bytes ({original_file_size/1024/1024:.1f} MB)")
        print(f"Final stream size:  {final_size:,} bytes ({final_size/1024/1024:.1f} MB)")
        print(f"Percentage served:  {percentage_served:.1f}%")
        
        if percentage_served < 95:
            print(f"❌ TRUNCATION DETECTED: Only {percentage_served:.1f}% of file was served")
            
            # Calculate estimated movie duration based on truncation
            if percentage_served > 0:
                # If original is 16min 20s (980 seconds) and we serve X%, estimate served duration
                original_duration_seconds = 16 * 60 + 20  # 16:20 from logs
                served_duration_seconds = (percentage_served / 100) * original_duration_seconds
                served_minutes = int(served_duration_seconds // 60)
                served_seconds = int(served_duration_seconds % 60)
                print(f"📺 Estimated served duration: {served_minutes}:{served_seconds:02d}")
        else:
            print("✅ Full file appears to be served")
        
        # Show growth rate analysis
        if len(self.results) > 1:
            total_time = self.results[-1]['elapsed_seconds'] - self.results[0]['elapsed_seconds']
            total_growth = self.results[-1]['stream_size_bytes'] - self.results[0]['stream_size_bytes']
            
            if total_time > 0:
                avg_rate = total_growth / total_time
                print(f"📈 Average growth rate: {avg_rate/1024:.1f} KB/s")
        
        # Find when growth stopped
        if len(self.results) >= 2:
            last_growth_time = None
            for i in range(len(self.results) - 1):
                if self.results[i+1]['growth_bytes'] > 0:
                    last_growth_time = self.results[i+1]['elapsed_seconds']
            
            if last_growth_time:
                print(f"⏰ Streaming stopped growing after: {last_growth_time:.0f} seconds")
    
    def cleanup(self):
        """Clean up resources"""
        if self.server_process:
            print("🔄 Stopping server...")
            self.server_process.terminate()
            try:
                self.server_process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                print("💥 Force killing server...")
                self.server_process.kill()
    
    def run_test(self):
        """Run the complete test"""
        try:
            print("🎬 PROGRESSIVE STREAMING TRUNCATION REPRODUCTION")
            print("=" * 60)
            
            # Start server
            if not self.start_server():
                return False
            
            # Get torrent info
            torrent = self.get_torrents()
            if not torrent:
                return False
            
            info_hash = torrent['info_hash']
            
            # Make initial request to trigger streaming
            content_range, first_chunk_size = self.initial_stream_request(info_hash)
            if not content_range:
                print("❌ Failed to start streaming")
                return False
            
            # Extract original file size from logs (we know it's 135046574 bytes from server.log)
            original_file_size = 135046574  # From the logs: "Final file size determined: 135046574 bytes"
            
            print(f"📁 Original file size: {original_file_size:,} bytes ({original_file_size/1024/1024:.1f} MB)")
            
            # Monitor progressive growth
            final_size = self.monitor_progressive_growth(info_hash, duration_minutes=3)
            
            # Analyze results
            self.analyze_results(original_file_size)
            
            return True
            
        except KeyboardInterrupt:
            print("\n⚠️  Test interrupted by user")
            return False
        except Exception as e:
            print(f"❌ Test failed: {e}")
            return False
        finally:
            self.cleanup()

if __name__ == "__main__":
    test = ProgressiveStreamingTest()
    success = test.run_test()
    sys.exit(0 if success else 1)
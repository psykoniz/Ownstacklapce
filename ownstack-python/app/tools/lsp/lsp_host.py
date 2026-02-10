
import argparse
import logging
import os
import signal
import socket
import socketserver
import subprocess
import sys
import threading
import time

# Configure logging
logging.basicConfig(
    level=logging.INFO,
    format="%(asctime)s [%(levelname)s] %(message)s",
    handlers=[logging.StreamHandler(sys.stderr)]
)
logger = logging.getLogger("lsp_host")

class LSPHandler(socketserver.BaseRequestHandler):
    """
    Handles a single connection from lsp_connect.py.
    Forwards the request to the LSP process and returns the response.
    """
    def handle(self):
        # Notify activity to reset TTL
        self.server.last_activity = time.time()
        
        try:
            # Read all data from socket (the full JSON-RPC payload)
            # The client sends the full request and waits for response.
            # In a real async LSP context, messages can be interleaved, 
            # but here we rely on the request-response nature of our connector.
            # For simplicity in this v1, we read until EOF (client closes write side)
            # or we implement proper framing reading. 
            # To be robust, we read the Content-Length header.
            
            # Read headers
            header = b""
            while b"\r\n\r\n" not in header:
                chunk = self.request.recv(1)
                if not chunk:
                    break
                header += chunk
            
            if b"\r\n\r\n" not in header:
                return

            # Extract content length
            content_length = 0
            headers_str = header.decode("ascii")
            for line in headers_str.split("\r\n"):
                if line.startswith("Content-Length:"):
                    content_length = int(line.split(":")[1].strip())
            
            # Read body
            body = b""
            while len(body) < content_length:
                chunk = self.request.recv(min(4096, content_length - len(body)))
                if not chunk:
                    break
                body += chunk
            
            full_message = header + body
            
            # Forward to LSP Process
            with self.server.lsp_lock:
                process = self.server.lsp_process
                if process.poll() is not None:
                    logger.error("LSP process has died.")
                    return

                try:
                    process.stdin.write(full_message)
                    process.stdin.flush()
                except BrokenPipeError:
                    logger.error("Broken pipe to LSP process.")
                    return

                # Read response
                # We need to read *one* JSON-RPC message back.
                # NOTE: This assumes the LSP replies with exactly one message per request.
                # In complex scenarios (diagnostics push), this might be insufficient,
                # but for request/response (hover, def), it fits.
                
                resp_header = b""
                while b"\r\n\r\n" not in resp_header:
                    char = process.stdout.read(1)
                    if not char:
                        break
                    resp_header += char
                
                if b"\r\n\r\n" not in resp_header:
                    return

                # Parse length
                resp_len = 0
                for line in resp_header.decode("ascii").split("\r\n"):
                    if line.startswith("Content-Length:"):
                        resp_len = int(line.split(":")[1].strip())
                
                resp_body = process.stdout.read(resp_len)
                
                # Send back to socket
                self.request.sendall(resp_header + resp_body)

        except Exception as e:
            logger.error(f"Error handling request: {e}")

class LSPHostServer(socketserver.ThreadingUnixStreamServer):
    def __init__(self, server_address, RequestHandlerClass, command, ttl):
        self.allow_reuse_address = True
        super().__init__(server_address, RequestHandlerClass)
        self.command = command
        self.ttl = ttl
        self.last_activity = time.time()
        self.lsp_lock = threading.Lock()
        
        # Start LSP Process
        logger.info(f"Starting LSP: {command}")
        self.lsp_process = subprocess.Popen(
            command,
            shell=True,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=sys.stderr, # Redirect stderr to host stderr
            bufsize=0 # Unbuffered I/O
        )

    def service_actions(self):
        """Called by the serve_forever loop periodically."""
        # Check TTL
        if time.time() - self.last_activity > self.ttl:
            logger.info(f"TTL expired ({self.ttl}s). Shutting down.")
            self.shutdown()
            self.server_close() # Force close
            
        # Check if process is still alive
        if self.lsp_process.poll() is not None:
            logger.error("LSP process exited unexpectedly.")
            self.shutdown()
            self.server_close()

    def server_close(self):
        try:
            if self.lsp_process.poll() is None:
                self.lsp_process.terminate()
                self.lsp_process.wait(timeout=2)
        except:
            pass
        # Clean up socket file
        try:
            os.unlink(self.server_address)
        except OSError:
            pass
        super().server_close()

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--socket", required=True, help="Unix socket path")
    parser.add_argument("--command", required=True, help="LSP start command")
    parser.add_argument("--ttl", type=int, default=300, help="Idle timeout in seconds")
    args = parser.parse_args()

    # Ensure socket directory exists
    socket_dir = os.path.dirname(args.socket)
    if not os.path.exists(socket_dir):
        os.makedirs(socket_dir, exist_ok=True)
        
    # Remove stale socket
    if os.path.exists(args.socket):
        os.unlink(args.socket)

    server = LSPHostServer(args.socket, LSPHandler, args.command, args.ttl)
    
    # Handle signals
    def signal_handler(sig, frame):
        logger.info("Received signal, shutting down...")
        server.shutdown()
        server.server_close()
        sys.exit(0)
        
    signal.signal(signal.SIGTERM, signal_handler)
    signal.signal(signal.SIGINT, signal_handler)

    logger.info(f"LSP Host listening on {args.socket}")
    # We must use a separate thread for the server loop to allow signal handling usually,
    # but handle_request loop or serve_forever with daemon threads works.
    # serve_forever blocks, so we can't check TTL easily unless we use timeout/service_actions.
    # ThreadingUnixStreamServer doesn't support 'service_actions' in older python versions,
    # but in 3.10+ it typically works if we override socketserver.BaseServer methods?
    # Actually BaseServer.serve_forever calls service_actions().
    
    server.serve_forever(poll_interval=1.0)

if __name__ == "__main__":
    main()

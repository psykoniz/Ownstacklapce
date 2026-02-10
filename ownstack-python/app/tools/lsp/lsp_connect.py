
import argparse
import socket
import sys

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--socket", required=True, help="Unix socket path")
    args = parser.parse_args()

    try:
        # Read payload from stdin (binary)
        # In Python 3, sys.stdin.buffer reads raw bytes
        payload = sys.stdin.buffer.read()
        
        if not payload:
            sys.exit(0)

        # Connect to host
        with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as sock:
            sock.settimeout(10.0) # 10s timeout for connection/response
            sock.connect(args.socket)
            
            # Send payload
            sock.sendall(payload)
            
            # Read response
            # We read until EOF because the host closes the connection after sending response
            # (or we rely on the host implementation which keeps it open? 
            # In lsp_host.py I used BaseRequestHandler which handles ONE request and then closes?
            # Wait, BaseRequestHandler usually handles one connection. 
            # If lsp_host reads loop, it keeps open.
            # But my lsp_host implementation reads once and "returns" from handle(), which closes the request.
            # So yes, read until EOF is correct.)
            
            response = b""
            while True:
                chunk = sock.recv(4096)
                if not chunk:
                    break
                response += chunk
            
            sys.stdout.buffer.write(response)

    except Exception as e:
        # Connection failed or timeout -> Fallback needed
        print(f"LSP Connect Error: {e}", file=sys.stderr)
        sys.exit(1)

if __name__ == "__main__":
    main()

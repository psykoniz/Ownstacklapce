import sys
import json
import logging

# Configure logging to stderr so it doesn't interfere with stdout JSON-RPC
logging.basicConfig(stream=sys.stderr, level=logging.INFO)

def read_message():
    content_length = 0
    while True:
        line = sys.stdin.readline()
        if not line:
            return None
        line = line.strip()
        if not line:
            break
        if line.startswith("Content-Length: "):
            content_length = int(line.split(": ")[1])
    
    if content_length == 0:
        return None
    
    body = sys.stdin.read(content_length)
    return json.loads(body)

def send_message(msg):
    body = json.dumps(msg)
    content_length = len(body)
    sys.stdout.write(f"Content-Length: {content_length}\r\n\r\n{body}")
    sys.stdout.flush()

def main():
    logging.info("Mock LSP server started")
    while True:
        try:
            msg = read_message()
            if msg is None:
                break
            
            logging.info(f"Received: {msg}")

            if "id" in msg:
                # Request
                method = msg.get("method")
                req_id = msg.get("id")
                
                response = {
                    "jsonrpc": "2.0",
                    "id": req_id,
                    "result": None
                }

                if method == "initialize":
                    response["result"] = {
                        "capabilities": {
                            "textDocumentSync": 1,
                            "hoverProvider": True
                        }
                    }
                elif method == "textDocument/hover":
                    response["result"] = {
                        "contents": "Hover content from mock server"
                    }
                elif method == "shutdown":
                    response["result"] = None
                
                send_message(response)
                
            else:
                # Notification
                method = msg.get("method")
                if method == "exit":
                    break
                elif method == "initialized":
                    # Send a diagnostic for fun
                    notification = {
                        "jsonrpc": "2.0",
                        "method": "textDocument/publishDiagnostics",
                        "params": {
                            "uri": "file:///workspace/test.rs",
                            "diagnostics": [
                                {
                                    "range": {
                                        "start": {"line": 0, "character": 0},
                                        "end": {"line": 0, "character": 1}
                                    },
                                    "severity": 1,
                                    "message": "Mock diagnostic error"
                                }
                            ]
                        }
                    }
                    send_message(notification)

        except Exception as e:
            logging.error(f"Error: {e}")
            break

if __name__ == "__main__":
    main()

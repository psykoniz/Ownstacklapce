#!/usr/bin/env python3
"""
Quick test script for the OwnStack Python bridge.
Tests basic JSON-RPC communication.
"""

import json
import sys

def handle_request(request):
    """Handle a single JSON-RPC request."""
    method = request.get("method")
    params = request.get("params", {})
    request_id = request.get("id")
    
    if method == "ping":
        return {
            "jsonrpc": "2.0",
            "id": request_id,
            "result": {"status": "pong", "message": "OwnStack bridge is alive!"}
        }
    elif method == "ai_prompt":
        prompt = params.get("prompt", "")
        return {
            "jsonrpc": "2.0",
            "id": request_id,
            "result": {
                "response": f"Received prompt: {prompt}",
                "status": "success"
            }
        }
    else:
        return {
            "jsonrpc": "2.0",
            "id": request_id,
            "error": {
                "code": -32601,
                "message": f"Method not found: {method}"
            }
        }

def main():
    """Main loop: read JSON-RPC requests from stdin, write responses to stdout."""
    for line in sys.stdin:
        try:
            request = json.loads(line.strip())
            response = handle_request(request)
            print(json.dumps(response), flush=True)
        except json.JSONDecodeError as e:
            error_response = {
                "jsonrpc": "2.0",
                "id": None,
                "error": {
                    "code": -32700,
                    "message": f"Parse error: {str(e)}"
                }
            }
            print(json.dumps(error_response), flush=True)
        except Exception as e:
            error_response = {
                "jsonrpc": "2.0",
                "id": None,
                "error": {
                    "code": -32603,
                    "message": f"Internal error: {str(e)}"
                }
            }
            print(json.dumps(error_response), flush=True)

if __name__ == "__main__":
    main()

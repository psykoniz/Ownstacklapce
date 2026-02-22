import json
import sys
import traceback
from typing import Any, Optional

CONTRACT_NAME = "ownstack.bridge.jsonio"
CONTRACT_VERSION = 1
JSONRPC_VERSION = "2.0"


def contract_payload() -> dict[str, Any]:
    return {"name": CONTRACT_NAME, "version": CONTRACT_VERSION}


def success_response(request_id: Optional[int], result: Any) -> dict[str, Any]:
    return {
        "jsonrpc": JSONRPC_VERSION,
        "id": request_id,
        "result": result,
        "error": None,
        "contract": contract_payload(),
    }


def error_response(
    request_id: Optional[int],
    code: int,
    message: str,
    data: Any = None,
) -> dict[str, Any]:
    error: dict[str, Any] = {"code": code, "message": message}
    if data is not None:
        error["data"] = data
    return {
        "jsonrpc": JSONRPC_VERSION,
        "id": request_id,
        "result": None,
        "error": error,
        "contract": contract_payload(),
    }


def validate_contract(contract: Any) -> Optional[str]:
    if contract is None:
        return "Missing contract metadata"

    if not isinstance(contract, dict):
        return "Invalid contract payload type"

    name = contract.get("name")
    version = contract.get("version")

    if name != CONTRACT_NAME:
        return f"Unsupported contract name: {name}"

    if version != CONTRACT_VERSION:
        return f"Unsupported contract version: {version}"

    return None


def handle_request(line: str) -> str:
    try:
        request = json.loads(line)
    except json.JSONDecodeError as exc:
        return json.dumps(
            error_response(None, -32700, "Parse error", {"detail": str(exc)})
        )

    request_id = request.get("id")

    if request.get("jsonrpc") != JSONRPC_VERSION:
        return json.dumps(
            error_response(
                request_id,
                -32600,
                "Invalid JSON-RPC version",
                {"expected": JSONRPC_VERSION},
            )
        )

    contract_error = validate_contract(request.get("contract"))
    if contract_error:
        return json.dumps(error_response(request_id, -32001, contract_error))

    method = request.get("method")
    if not isinstance(method, str) or not method.strip():
        return json.dumps(error_response(request_id, -32600, "Invalid method"))

    params = request.get("params", {})
    if params is None:
        params = {}

    # Placeholder routing until all Python-side tools are fully migrated to Rust.
    result = {"status": "received", "method": method, "params": params}
    return json.dumps(success_response(request_id, result))


def main() -> None:
    for line in sys.stdin:
        if not line.strip():
            continue
        try:
            response = handle_request(line)
        except Exception as exc:  # pragma: no cover - safety net for malformed runtime state
            response = json.dumps(
                error_response(
                    None,
                    -32603,
                    "Internal bridge failure",
                    {"detail": str(exc), "traceback": traceback.format_exc()},
                )
            )
        sys.stdout.write(response + "\n")
        sys.stdout.flush()


if __name__ == "__main__":
    main()

"""RepoMap v2 - Enhanced Tree-sitter symbol extraction with deep code understanding.

Features over v1:
- [TODO] In-process Tree-sitter parsing (Currently uses CLI subprocess - see Audit P1)
- Extended queries: imports, exports, docstrings, decorators
- Method signatures with parameters and return types
- Call graph extraction for understanding dependencies
- Scope-aware symbol nesting (methods inside classes)

NOTE: This implementation relies on `repomap_runner.py` invoking `tree-sitter` CLI.
True in-process bindings are planned for V3 to remove subprocess overhead.
"""
from __future__ import annotations

import re
from dataclasses import dataclass, field
from typing import Any, Dict, Iterable, List, Optional, Set


@dataclass(frozen=True)
class Capture:
    """Represents a Tree-sitter capture with position info."""
    name: str
    start_line: int
    start_col: int
    end_line: int
    end_col: int


@dataclass
class Symbol:
    """Enhanced symbol with full metadata."""
    name: str
    kind: str  # class, function, method, import, variable, etc.
    line: int
    signature: Optional[str] = None
    docstring: Optional[str] = None
    decorators: List[str] = field(default_factory=list)
    children: List["Symbol"] = field(default_factory=list)
    references: List[str] = field(default_factory=list)  # calls to other symbols
    
    def to_dict(self) -> Dict[str, Any]:
        result: Dict[str, Any] = {"name": self.name, "kind": self.kind, "line": self.line}
        if self.signature:
            result["signature"] = self.signature
        if self.docstring:
            result["docstring"] = self.docstring[:200]  # Truncate long docstrings
        if self.decorators:
            result["decorators"] = self.decorators
        if self.children:
            result["children"] = [c.to_dict() for c in self.children]
        if self.references:
            result["references"] = self.references
        return result


# Extended queries with more symbol types
QUERIES_V2: Dict[str, str] = {
    "python": """
        ; Classes with decorators and docstrings
        (decorated_definition
            (decorator (identifier) @decorator)
            definition: (class_definition 
                name: (identifier) @class.name
                body: (block . (expression_statement (string) @class.docstring)?)
            )
        ) @class.decorated
        
        (class_definition 
            name: (identifier) @class
            body: (block . (expression_statement (string) @class.docstring)?)
        )
        
        ; Functions with decorators, parameters, and docstrings  
        (decorated_definition
            (decorator (identifier) @decorator)
            definition: (function_definition 
                name: (identifier) @function.name
                parameters: (parameters) @function.params
                return_type: (type)? @function.return_type
                body: (block . (expression_statement (string) @function.docstring)?)
            )
        ) @function.decorated
        
        (function_definition 
            name: (identifier) @function
            parameters: (parameters) @params
            return_type: (type)? @return_type
            body: (block . (expression_statement (string) @docstring)?)
        )
        
        ; Imports
        (import_statement (dotted_name) @import)
        (import_from_statement 
            module_name: (dotted_name) @import.module
            (aliased_import name: (identifier) @import.name)?
            (identifier) @import.name
        )
        
        ; Function calls (for call graph)
        (call function: (identifier) @call)
        (call function: (attribute attribute: (identifier) @call.method))
        
        ; Top-level assignments
        (expression_statement
            (assignment left: (identifier) @variable)
        )
    """,
    
    "typescript": """
        ; Classes
        (class_declaration 
            name: (type_identifier) @class
        )
        (abstract_class_declaration
            name: (type_identifier) @class.abstract
        )
        
        ; Functions
        (function_declaration 
            name: (identifier) @function
            parameters: (formal_parameters) @params
            return_type: (type_annotation)? @return_type
        )
        (arrow_function) @arrow
        
        ; Methods
        (method_definition
            name: (property_identifier) @method
            parameters: (formal_parameters) @params
        )
        
        ; Interfaces and types
        (interface_declaration name: (type_identifier) @interface)
        (type_alias_declaration name: (type_identifier) @type)
        (enum_declaration name: (identifier) @enum)
        
        ; Imports/Exports
        (import_statement
            (import_clause (identifier) @import.default)?
            (import_clause (named_imports (import_specifier name: (identifier) @import.named)))?
            source: (string) @import.from
        )
        (export_statement
            (export_clause (export_specifier name: (identifier) @export))?
            declaration: (lexical_declaration 
                (variable_declarator name: (identifier) @export.var))?
            declaration: (function_declaration name: (identifier) @export.function)?
        )
        
        ; Variables
        (lexical_declaration (variable_declarator name: (identifier) @variable))
    """,
    
    "javascript": """
        ; Classes
        (class_declaration name: (identifier) @class)
        
        ; Functions
        (function_declaration name: (identifier) @function)
        (arrow_function) @arrow
        
        ; Methods
        (method_definition name: (property_identifier) @method)
        
        ; Imports/Exports
        (import_statement
            (import_clause (identifier) @import.default)?
            (import_clause (named_imports (import_specifier name: (identifier) @import.named)))?
            source: (string) @import.from
        )
        (export_statement
            (export_clause (export_specifier name: (identifier) @export))?
        )
        
        ; Variables
        (lexical_declaration (variable_declarator name: (identifier) @variable))
    """,
    
    "cpp": """
        ; Classes and structs
        (class_specifier name: (type_identifier) @class)
        (struct_specifier name: (type_identifier) @struct)
        
        ; Namespaces
        (namespace_definition name: (namespace_identifier) @namespace)
        
        ; Functions
        (function_definition 
            declarator: (function_declarator 
                declarator: (identifier) @function
                parameters: (parameter_list) @params
            )
        )
        
        ; Methods
        (function_definition
            declarator: (function_declarator
                declarator: (qualified_identifier
                    scope: (namespace_identifier) @method.class
                    name: (identifier) @method
                )
            )
        )
        
        ; Includes
        (preproc_include path: (string_literal) @include)
        (preproc_include path: (system_lib_string) @include.system)
    """,
}


CAPTURE_PATTERNS = [
    re.compile(r"^(?P<name>\w+(?:\.\w+)?)\s+\[(?P<sl>\d+),(?P<sc>\d+)\]\s+-\s+\[(?P<el>\d+),(?P<ec>\d+)\]"),
    re.compile(r"^(?P<name>\w+(?:\.\w+)?)\s+(?P<sl>\d+):(?P<sc>\d+)\s+-\s+(?P<el>\d+):(?P<ec>\d+)"),
]


def query_for_language_v2(language: str) -> str:
    """Get the v2 enhanced query for a language."""
    return QUERIES_V2.get(language, "")


def parse_captures(output: str) -> Iterable[Capture]:
    """Parse tree-sitter --captures output into Capture objects."""
    for line in output.splitlines():
        line = line.strip()
        if not line:
            continue
        capture = _parse_line(line)
        if capture:
            yield capture


def _parse_line(line: str) -> Optional[Capture]:
    for pattern in CAPTURE_PATTERNS:
        match = pattern.match(line)
        if match:
            return Capture(
                name=match.group("name"),
                start_line=int(match.group("sl")),
                start_col=int(match.group("sc")),
                end_line=int(match.group("el")),
                end_col=int(match.group("ec")),
            )
    return None


def extract_symbols_v2(source: str, captures: Iterable[Capture]) -> List[Symbol]:
    """
    Extract enhanced symbols from captures.
    Groups related captures (function + params + docstring) into single Symbol objects.
    """
    lines = source.splitlines()
    symbols: List[Symbol] = []
    current_class: Optional[Symbol] = None
    
    capture_list = list(captures)
    
    for i, capture in enumerate(capture_list):
        text = _slice_text(lines, capture)
        if not text:
            continue
        
        kind = capture.name.split(".")[0]  # Get base kind (e.g., "function" from "function.name")
        
        if kind == "class":
            current_class = Symbol(name=text, kind="class", line=capture.start_line)
            symbols.append(current_class)
        elif kind == "function":
            sym = Symbol(name=text, kind="function", line=capture.start_line)
            # Look for associated params
            sym.signature = _find_nearby_signature(capture_list, i, lines, "params")
            if current_class and capture.start_line > current_class.line:
                # Check if this is a method (indented inside class)
                sym.kind = "method"
                current_class.children.append(sym)
            else:
                symbols.append(sym)
        elif kind == "method":
            sym = Symbol(name=text, kind="method", line=capture.start_line)
            if current_class:
                current_class.children.append(sym)
            else:
                symbols.append(sym)
        elif kind == "import":
            symbols.append(Symbol(name=text, kind="import", line=capture.start_line))
        elif kind == "export":
            symbols.append(Symbol(name=text, kind="export", line=capture.start_line))
        elif kind in ("interface", "type", "enum", "struct", "namespace"):
            symbols.append(Symbol(name=text, kind=kind, line=capture.start_line))
        elif kind == "call":
            # Track calls for call graph
            if symbols:
                # Add to most recent function/method
                for s in reversed(symbols):
                    if s.kind in ("function", "method"):
                        s.references.append(text)
                        break
        elif kind == "variable":
            symbols.append(Symbol(name=text, kind="variable", line=capture.start_line))
        elif kind == "decorator":
            if symbols:
                symbols[-1].decorators.append(text)
    
    return symbols


def _find_nearby_signature(
    captures: List[Capture], 
    current_idx: int, 
    lines: List[str], 
    target_kind: str
) -> Optional[str]:
    """Find a nearby capture of a specific kind and extract its text."""
    for j in range(current_idx + 1, min(current_idx + 5, len(captures))):
        other = captures[j]
        if other.name == target_kind or other.name.endswith(f".{target_kind}"):
            return _slice_text(lines, other)
    return None


def _slice_text(lines: List[str], capture: Capture) -> str:
    """Extract text from source lines using capture position."""
    if capture.start_line >= len(lines) or capture.end_line >= len(lines):
        return ""
    if capture.start_line == capture.end_line:
        line = lines[capture.start_line]
        return line[capture.start_col:capture.end_col] if capture.end_col <= len(line) else ""
    # Multi-line capture
    result = lines[capture.start_line][capture.start_col:]
    for i in range(capture.start_line + 1, capture.end_line):
        result += "\n" + lines[i]
    result += "\n" + lines[capture.end_line][:capture.end_col]
    return result


def symbols_to_dict(symbols: List[Symbol]) -> List[Dict[str, Any]]:
    """Convert Symbol list to serializable dict format."""
    return [s.to_dict() for s in symbols]


def build_call_graph(symbols: List[Symbol]) -> Dict[str, Set[str]]:
    """
    Build a call graph from extracted symbols.
    Returns mapping of function_name -> set of functions it calls.
    """
    graph: Dict[str, Set[str]] = {}
    
    for sym in symbols:
        if sym.kind in ("function", "method"):
            graph[sym.name] = set(sym.references)
        for child in sym.children:
            if child.kind in ("function", "method"):
                full_name = f"{sym.name}.{child.name}"
                graph[full_name] = set(child.references)
    
    return graph

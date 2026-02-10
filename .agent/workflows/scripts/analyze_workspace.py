import os
import re
import sys
from pathlib import Path

def analyze_rust_workspace(root_dir):
    root = Path(root_dir)
    cargo_toml = root / "Cargo.toml"
    
    if not cargo_toml.exists():
        print(f"Error: No Cargo.toml found in {root_dir}")
        return

    content = cargo_toml.read_text()
    
    # Simple regex to find workspace members
    members_match = re.search(r'members\s*=\s*\[(.*?)\]', content, re.DOTALL)
    if not members_match:
        print("No workspace members found.")
        return

    members_str = members_match.group(1)
    members = [m.strip().strip('"').strip("'") for m in members_str.split(',') if m.strip()]

    print(f"Codebase Analysis for: {root.name}\n")
    print(f"{'Crate':<20} | {'Type':<8} | {'Description'}")
    print("-" * 60)

    for member in members:
        member_path = root / member
        member_toml = member_path / "Cargo.toml"
        
        crate_type = "Unknown"
        description = "No description"
        
        if member_toml.exists():
            toml_content = member_toml.read_text()
            
            # Check if it's a lib or bin
            if (member_path / "src" / "lib.rs").exists():
                crate_type = "Library"
            elif (member_path / "src" / "main.rs").exists() or (member_path / "src" / "bin").exists():
                crate_type = "Binary"
            
            # Extract description
            desc_match = re.search(r'description\s*=\s*"(.*?)"', toml_content)
            if desc_match:
                description = desc_match.group(1)
        
        print(f"{member:<20} | {crate_type:<8} | {description}")

    print("\nEntry Points Mapping:")
    for member in members:
        member_path = root / member
        src_path = member_path / "src"
        if src_path.exists():
            entry_points = []
            if (src_path / "lib.rs").exists():
                entry_points.append("src/lib.rs")
            if (src_path / "main.rs").exists():
                entry_points.append("src/main.rs")
            
            bin_dir = src_path / "bin"
            if bin_dir.exists():
                for f in bin_dir.glob("*.rs"):
                    entry_points.append(f"src/bin/{f.name}")
            
            if entry_points:
                print(f"- {member}: {', '.join(entry_points)}")

if __name__ == "__main__":
    analyze_rust_workspace(".")

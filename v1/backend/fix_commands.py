#!/usr/bin/env python3
"""Migrate Result<_, String> to Result<_, CoreError> in command modules."""

import re
import sys

CORERROR_IMPORT = "use crate::core::error::CoreError;"


def fix_return_types(content):
    """Pass 1: Fix -> Result<X, String> to -> Result<X, CoreError>"""
    return re.sub(r'->\s*Result<(.+?),\s*String>', r'-> Result<\1, CoreError>', content)


def fix_map_err_format(content):
    """Pass 2: Fix .map_err(|e| format!(...)) to wrap with CoreError::from"""
    pattern = r'\.map_err\(\|e\|\s*format!\(([^()]*(?:\([^()]*\)[^()]*)*)\)\s*\)'
    replacement = r'.map_err(|e| CoreError::from(format!(\1)))'
    return re.sub(pattern, replacement, content)


def fix_map_err_to_string(content):
    """Pass 3: Fix .map_err(|e| e.to_string()) to wrap with CoreError::from"""
    pattern = r'\.map_err\(\|e\|\s*e\.to_string\(\)\)'
    replacement = r'.map_err(|e| CoreError::from(e.to_string()))'
    return re.sub(pattern, replacement, content)


def fix_map_err_var_to_string(content):
    """Pass 3b: Fix .map_err(|var| var.to_string()) with any variable"""
    pattern = r'\.map_err\(\|(\w+)\|\s*(\1)\.to_string\(\)\)'
    replacement = r'.map_err(|\1| CoreError::from(\1.to_string()))'
    return re.sub(pattern, replacement, content)


def fix_err_format(content):
    """Pass 4: Fix Err(format!(...)) to Err(CoreError::from(format!(...)))"""
    pattern = r'Err\(format!\(([^()]*(?:\([^()]*\)[^()]*)*)\)\)'
    replacement = r'Err(CoreError::from(format!(\1)))'
    return re.sub(pattern, replacement, content)


def fix_err_string(content):
    """Pass 4b: Fix Err("...".to_string()) to Err(CoreError::from("...".to_string()))"""
    pattern = r'Err\(("[^"]*"\.to_string\(\))\)(?!\))'
    replacement = r'Err(CoreError::from(\1))'
    return re.sub(pattern, replacement, content)


def fix_project_error_into(content):
    """Pass 4c: Wrap ProjectError::Something(...).to_string() with CoreError::from(...)
    
    Patterns:
      return Err(ProjectError::Foo(...).to_string());    → CoreError::from(...)
      .map_err(|e| ProjectError::Foo(...).to_string())   → CoreError::from(...)
      .ok_or_else(|| ProjectError::Foo(...).to_string())  → CoreError::from(...)
    """
    pattern = r'(ProjectError::\w+\([^()]*(?:\([^()]*\)[^()]*)*\)\s*\.to_string\(\))'
    replacement = r'CoreError::from(\1)'
    return re.sub(pattern, replacement, content)


def add_coreerror_import(content):
    """Pass 5: Add CoreError import if not already present at top level.
    
    Uses brace-depth tracking to properly handle multi-line use blocks like:
        use crate::core::foo::{
            ItemA,
            ItemB,
        };
    """
    if CORERROR_IMPORT in content:
        return content

    lines = content.split('\n')

    import_section_end = 0
    in_block = False
    block_depth = 0

    for i, line in enumerate(lines):
        stripped = line.strip()

        # Skip blank lines and comment-only lines (they can appear anywhere)
        if not stripped or stripped.startswith('//') or stripped.startswith('/*') or stripped.startswith('*'):
            continue

        if in_block:
            # Currently inside a multi-line use { } block
            block_depth += stripped.count('{') - stripped.count('}')
            if block_depth <= 0:
                in_block = False
                import_section_end = i + 1  # The block just ended
            continue

        # Indented lines at top-level scope are continuations
        if line and line[0] in (' ', '\t'):
            continue

        # Top-level import/mod/attribute lines
        if stripped.startswith('use ') or stripped.startswith('mod ') or stripped.startswith('#!'):
            import_section_end = i + 1
            if '{' in stripped:
                depth = stripped.count('{') - stripped.count('}')
                if depth > 0:
                    in_block = True
                    block_depth = depth
                # else: { and } on same line, block fully resolved, stay out of in_block
        else:
            # First top-level non-import declaration
            break

    lines.insert(import_section_end, CORERROR_IMPORT)
    return '\n'.join(lines)


def fix_file(filepath):
    with open(filepath, 'r', encoding='utf-8') as f:
        content = f.read()

    original = content

    content = fix_return_types(content)
    content = fix_map_err_format(content)
    content = fix_map_err_to_string(content)
    content = fix_map_err_var_to_string(content)
    content = fix_err_format(content)
    content = fix_err_string(content)
    content = fix_project_error_into(content)
    content = add_coreerror_import(content)

    if content != original:
        with open(filepath, 'w', encoding='utf-8') as f:
            f.write(content)
        return True
    return False


def main():
    files = sys.argv[1:] if len(sys.argv) > 1 else [
        "src/commands/driver_commands.rs",
        "src/commands/mock_commands.rs",
        "src/commands/mock_persistence_commands.rs",
        "src/commands/scratchpad_commands.rs",
        "src/commands/project_store_commands.rs",
        "src/commands/project_commands.rs",
        "src/commands/port_commands.rs",
        "src/commands/performance_commands.rs",
        "src/commands/navigator_commands.rs",
        "src/commands/metadata_cache_commands.rs",
        "src/commands/memory_commands.rs",
        "src/commands/logging_commands.rs",
    ]

    for f in files:
        changed = fix_file(f)
        print(f"{'CHANGED' if changed else 'no change':8s}  {f}")


if __name__ == '__main__':
    main()
#!/bin/bash
# SPDX-License-Identifier: MIT
# Copyright (c) 2025-2026 Jeff Garzik
#
# Reorder Rust file headers to: doc comment block, blank, copyright, SPDX
#
# Expected OUTPUT format:
#   //! Module description
#   //! ... (rest of doc comment)
#
#   // Copyright (c) 2025-2026 Jeff Garzik
#   // SPDX-License-Identifier: MIT
#
#   use ...

set -e

SPDX_LINE="// SPDX-License-Identifier: MIT"
COPYRIGHT_LINE="// Copyright (c) 2025-2026 Jeff Garzik"

fix_file() {
    local file="$1"
    local tmpfile=$(mktemp)

    # Check if file starts with doc comment
    local first_line=$(head -1 "$file")
    if [[ ! "$first_line" =~ ^//! ]]; then
        echo "Skipping $file (doesn't start with //! doc comment)"
        return
    fi

    # Check if file has our copyright/SPDX (in either order)
    if ! grep -q "SPDX-License-Identifier: MIT" "$file"; then
        echo "Skipping $file (no SPDX header found)"
        return
    fi

    # Extract all doc comment lines from the start
    local doc_comment=""
    local in_doc=true
    local line_num=0
    local after_doc=""

    while IFS= read -r line || [[ -n "$line" ]]; do
        line_num=$((line_num + 1))
        if $in_doc; then
            if [[ "$line" =~ ^//! ]] || [[ -z "$line" && $line_num -eq 1 ]]; then
                doc_comment+="$line"$'\n'
            elif [[ -z "$line" ]]; then
                # Blank line - could be end of doc comment or within it
                # Peek ahead: if next non-blank line is //!, continue doc comment
                doc_comment+="$line"$'\n'
            else
                in_doc=false
                after_doc+="$line"$'\n'
            fi
        else
            after_doc+="$line"$'\n'
        fi
    done < "$file"

    # Remove trailing newlines from doc_comment, then trim any embedded copyright/SPDX
    doc_comment=$(echo "$doc_comment" | grep -v "^// Copyright" | grep -v "^// SPDX")

    # Remove copyright/SPDX from the rest of the file
    after_doc=$(echo "$after_doc" | grep -v "^// Copyright" | grep -v "^// SPDX")

    # Remove leading blank lines from after_doc
    after_doc=$(echo "$after_doc" | sed '/./,$!d')

    # Write the reordered file
    {
        # Doc comment (trimmed of trailing blanks)
        echo "$doc_comment" | sed -e :a -e '/^\s*$/d;N;ba'
        echo ""
        echo "$COPYRIGHT_LINE"
        echo "$SPDX_LINE"
        echo ""
        # Rest of file
        echo "$after_doc"
    } > "$tmpfile"

    # Remove any double blank lines
    cat -s "$tmpfile" > "$file"
    rm "$tmpfile"

    echo "Fixed $file"
}

# Find all .rs files
if [[ $# -gt 0 ]]; then
    # Process specified files
    for file in "$@"; do
        fix_file "$file"
    done
else
    # Process all .rs files in src/ and tests/
    find src tests -name "*.rs" -type f | while read -r file; do
        fix_file "$file"
    done
fi

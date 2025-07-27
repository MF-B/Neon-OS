#!/bin/bash

AX_ROOT=arceos

test ! -d "$AX_ROOT" && echo "Cloning repositories ..." || true
test ! -d "$AX_ROOT" && git clone https://github.com/MF-B/arceos "$AX_ROOT" || true

# 切换到 final-2025 分支
git -C "$AX_ROOT" checkout final-2025 || {
    echo "Failed to checkout branch final-2025. Please check the repository."
    exit 1
}

$(dirname $0)/set_ax_root.sh $AX_ROOT

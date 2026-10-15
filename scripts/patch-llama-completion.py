#!/usr/bin/env python3
"""Pinned llama.cpp v0.6.0 completion fix: forward chat reasoning options.

The CLI parses --reasoning off but omitted these options when formatting the
initial completion prompt. Keep the small patch explicit and reproducible.
"""
from pathlib import Path
import sys

path = Path(sys.argv[1]) / "tools/completion/completion.cpp"
original = "                inputs.use_jinja = g_params->use_jinja;\n"
patched = original + "                inputs.enable_thinking = params.enable_reasoning != 0;\n                inputs.chat_template_kwargs = params.default_template_kwargs;\n"
source = path.read_text()
if patched not in source:
    if source.count(original) != 1:
        raise SystemExit("Pinned llama.cpp completion source changed; review the reasoning patch")
    path.write_text(source.replace(original, patched))

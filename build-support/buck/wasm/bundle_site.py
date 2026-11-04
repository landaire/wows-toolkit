"""Assemble a wasm-bindgen web app into a directory a static host can serve.

The JS glue and the module are named after a hash of their contents, so a
browser never runs a cached copy of an older build. index.html gets a module
script that loads both from under the public URL, with preload hints carrying
subresource integrity hashes.
"""

import argparse
import base64
import hashlib
import html
import shutil
from pathlib import Path


def content_name(stem: str, suffix: str, data: bytes) -> str:
    return "{}-{}{}".format(stem, hashlib.sha256(data).hexdigest()[:16], suffix)


def integrity(data: bytes) -> str:
    return "sha384-" + base64.b64encode(hashlib.sha384(data).digest()).decode("ascii")


def loader(public_url: str, js_name: str, js: bytes, wasm_name: str, wasm: bytes) -> str:
    js_url = html.escape(public_url + js_name)
    wasm_url = html.escape(public_url + wasm_name)
    return "\n".join(
        [
            '<link rel="modulepreload" href="{}" crossorigin="anonymous" integrity="{}">'.format(
                js_url, integrity(js)
            ),
            '<link rel="preload" href="{}" crossorigin="anonymous" integrity="{}" as="fetch" type="application/wasm">'.format(
                wasm_url, integrity(wasm)
            ),
            '<script type="module">',
            "import init from '{}';".format(js_url),
            "await init({{ module_or_path: '{}' }});".format(wasm_url),
            "</script>",
            "",
        ]
    )


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--bindgen", type=Path, required=True, help="wasm-bindgen output directory")
    parser.add_argument("--wasm", type=Path, required=True, help="the module to ship")
    parser.add_argument("--index", type=Path, required=True)
    parser.add_argument("--name", required=True, help="wasm-bindgen --out-name")
    parser.add_argument("--public-url", required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()

    public_url = args.public_url if args.public_url.endswith("/") else args.public_url + "/"
    js = (args.bindgen / (args.name + ".js")).read_bytes()
    wasm = args.wasm.read_bytes()
    js_name = content_name(args.name, ".js", js)
    wasm_name = content_name(args.name, "_bg.wasm", wasm)

    page = args.index.read_text(encoding="utf-8")
    if page.count("</head>") != 1:
        raise SystemExit("{} must contain exactly one </head>.".format(args.index))
    page = page.replace("</head>", loader(public_url, js_name, js, wasm_name, wasm) + "</head>")

    args.out.mkdir(parents=True, exist_ok=True)
    (args.out / js_name).write_bytes(js)
    (args.out / wasm_name).write_bytes(wasm)
    (args.out / "index.html").write_text(page, encoding="utf-8")

    # inline_js and module imports, which the glue loads relative to itself.
    snippets = args.bindgen / "snippets"
    if snippets.is_dir():
        shutil.copytree(snippets, args.out / "snippets")


if __name__ == "__main__":
    main()

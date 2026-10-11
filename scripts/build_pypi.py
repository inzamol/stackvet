#!/usr/bin/env python3
import os
import subprocess
import sys

def main():
    root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    os.chdir(root)
    out_dir = os.path.join(root, "dist", "pypi")
    os.makedirs(out_dir, exist_ok=True)

    print("Building binary wheel (.whl)...")
    subprocess.run(["maturin", "build", "--release", "--out", out_dir], check=True)

    print("Building source distribution (.tar.gz)...")
    subprocess.run(["maturin", "sdist", "--out", out_dir], check=True)

    print(f"\nArtifacts generated in {out_dir}:")
    for f in os.listdir(out_dir):
        print(f" - {f}")

if __name__ == "__main__":
    main()

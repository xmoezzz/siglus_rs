import tarfile
import os

def pack_project():
    home = os.path.expanduser("~")
    output_path = os.path.join(home, "Downloads", "se.tar.gz")
    
    targets = [
        "crates",
        ".gitignore",
        "Cargo.toml",
        "Cargo.lock",
        "README.md"
    ]
    
    with tarfile.open(output_path, "w:gz") as tar:
        for target in targets:
            if os.path.exists(target):
                tar.add(target)

if __name__ == "__main__":
    pack_project()
import base64
import hashlib
import io
import json
import os
import struct
import tarfile
import urllib.error
import urllib.request
import zipfile

BASE = os.environ["REGISTRY_BASE"]
TOKEN = os.environ["REGISTRY_TOKEN"]
VERSION = os.environ["PKG_VERSION"]
NPM_NAME = os.environ["PKG_NPM"]
PYPI_NAME = os.environ["PKG_PYPI"]
CARGO_NAME = os.environ["PKG_CARGO"]


def send(method, path, body, content_type):
    request = urllib.request.Request(
        BASE + path,
        data=body,
        method=method,
        headers={"Authorization": "token " + TOKEN, "Content-Type": content_type},
    )
    try:
        with urllib.request.urlopen(request) as response:
            return response.status, response.read().decode()
    except urllib.error.HTTPError as error:
        return error.code, error.read().decode()


def tar_gz(entries):
    buffer = io.BytesIO()
    with tarfile.open(fileobj=buffer, mode="w:gz") as archive:
        for name, content in entries.items():
            data = content.encode()
            info = tarfile.TarInfo(name)
            info.size = len(data)
            info.mtime = 0
            archive.addfile(info, io.BytesIO(data))
    return buffer.getvalue()


def publish_npm():
    tarball = tar_gz(
        {
            "package/package.json": json.dumps({"name": NPM_NAME, "version": VERSION, "main": "index.js"}),
            "package/index.js": "module.exports.greeting = () => 'hello from " + NPM_NAME + "';\n",
        }
    )
    filename = f"{NPM_NAME}-{VERSION}.tgz"
    document = {
        "_id": NPM_NAME,
        "name": NPM_NAME,
        "description": "test package",
        "dist-tags": {"latest": VERSION},
        "versions": {
            VERSION: {
                "name": NPM_NAME,
                "version": VERSION,
                "main": "index.js",
                "dist": {
                    "integrity": "sha512-" + base64.b64encode(hashlib.sha512(tarball).digest()).decode(),
                    "shasum": hashlib.sha1(tarball).hexdigest(),
                },
            }
        },
        "_attachments": {
            filename: {
                "content_type": "application/octet-stream",
                "data": base64.b64encode(tarball).decode(),
                "length": len(tarball),
            }
        },
    }
    return send("PUT", f"/npm/{NPM_NAME}", json.dumps(document).encode(), "application/json")


def multipart(fields, file_field, file_name, file_bytes):
    boundary = "----sbxboundary7d1f"
    parts = []
    for key, value in fields.items():
        parts.append(f'--{boundary}\r\nContent-Disposition: form-data; name="{key}"\r\n\r\n{value}\r\n'.encode())
    parts.append(
        f'--{boundary}\r\nContent-Disposition: form-data; name="{file_field}"; filename="{file_name}"\r\n'
        "Content-Type: application/octet-stream\r\n\r\n".encode()
        + file_bytes
        + b"\r\n"
    )
    parts.append(f"--{boundary}--\r\n".encode())
    return b"".join(parts), f"multipart/form-data; boundary={boundary}"


def publish_pypi():
    module = PYPI_NAME.replace("-", "_")
    dist_info = f"{module}-{VERSION}.dist-info"
    buffer = io.BytesIO()
    with zipfile.ZipFile(buffer, "w") as wheel:
        wheel.writestr(f"{module}/__init__.py", f"def greeting():\n    return 'hello from {PYPI_NAME}'\n")
        wheel.writestr(
            f"{dist_info}/METADATA",
            f"Metadata-Version: 2.1\nName: {PYPI_NAME}\nVersion: {VERSION}\nSummary: test package\n",
        )
        wheel.writestr(
            f"{dist_info}/WHEEL",
            "Wheel-Version: 1.0\nGenerator: sbx\nRoot-Is-Purelib: true\nTag: py3-none-any\n",
        )
        wheel.writestr(f"{dist_info}/RECORD", "")
    wheel_bytes = buffer.getvalue()
    fields = {
        ":action": "file_upload",
        "protocol_version": "1",
        "name": PYPI_NAME,
        "version": VERSION,
        "metadata_version": "2.1",
        "summary": "test package",
        "filetype": "bdist_wheel",
        "pyversion": "py3",
        "sha256_digest": hashlib.sha256(wheel_bytes).hexdigest(),
    }
    body, content_type = multipart(fields, "content", f"{module}-{VERSION}-py3-none-any.whl", wheel_bytes)
    return send("POST", "/pypi", body, content_type)


def publish_cargo():
    crate = tar_gz(
        {
            f"{CARGO_NAME}-{VERSION}/Cargo.toml": f'[package]\nname = "{CARGO_NAME}"\nversion = "{VERSION}"\nedition = "2021"\ndescription = "test package"\nlicense = "MIT"\n',
            f"{CARGO_NAME}-{VERSION}/src/lib.rs": "pub fn greeting() -> &'static str {\n    \"hello from sbx-hello-lib\"\n}\n",
        }
    )
    metadata = json.dumps(
        {
            "name": CARGO_NAME,
            "vers": VERSION,
            "deps": [],
            "features": {},
            "authors": [],
            "description": "test package",
            "license": "MIT",
            "links": None,
        }
    ).encode()
    body = struct.pack("<I", len(metadata)) + metadata + struct.pack("<I", len(crate)) + crate
    return send("PUT", "/cargo/api/v1/crates/new", body, "application/octet-stream")


for kind, publish in (("npm", publish_npm), ("pypi", publish_pypi), ("cargo", publish_cargo)):
    status, text = publish()
    print(kind, status, text[:200].replace("\n", " "))

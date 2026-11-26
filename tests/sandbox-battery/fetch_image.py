import hashlib
import io
import json
import sys
import tarfile
import urllib.parse
import urllib.request

REF = sys.argv[1]
PLATFORM_ARCH = sys.argv[2]
ACCEPT = ",".join(
    [
        "application/vnd.oci.image.index.v1+json",
        "application/vnd.docker.distribution.manifest.list.v2+json",
        "application/vnd.oci.image.manifest.v1+json",
        "application/vnd.docker.distribution.manifest.v2+json",
    ]
)


class DropAuthOnRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        redirected = super().redirect_request(req, fp, code, msg, headers, newurl)
        if redirected is not None:
            redirected.remove_header("Authorization")
        return redirected


OPENER = urllib.request.build_opener(DropAuthOnRedirect)


def split_ref(ref):
    name, _, tag = ref.partition(":")
    if "/" not in name:
        name = "library/" + name
    return name, tag or "latest"


def fetch(url, headers):
    request = urllib.request.Request(url, headers=headers)
    return OPENER.open(request, timeout=120)


def token_for(name):
    scope = urllib.parse.quote(f"repository:{name}:pull")
    url = f"https://auth.docker.io/token?service=registry.docker.io&scope={scope}"
    with fetch(url, {}) as response:
        return json.load(response)["token"]


def manifest(name, reference, token):
    url = f"https://registry-1.docker.io/v2/{name}/manifests/{reference}"
    with fetch(url, {"Authorization": "Bearer " + token, "Accept": ACCEPT}) as response:
        return response.read()


def add_blob(archive, name, token, digest, size):
    url = f"https://registry-1.docker.io/v2/{name}/blobs/{digest}"
    info = tarfile.TarInfo("blobs/sha256/" + digest.split(":")[1])
    info.size = size
    with fetch(url, {"Authorization": "Bearer " + token}) as response:
        archive.addfile(info, response)


def add_bytes(archive, path, data):
    info = tarfile.TarInfo(path)
    info.size = len(data)
    archive.addfile(info, io.BytesIO(data))


name, tag = split_ref(REF)
token = token_for(name)
top = manifest(name, tag, token)
top_digest = "sha256:" + hashlib.sha256(top).hexdigest()
document = json.loads(top)
selected = top
selected_digest = top_digest
if "manifests" in document:
    chosen = next(
        entry
        for entry in document["manifests"]
        if entry["platform"]["os"] == "linux" and entry["platform"]["architecture"] == PLATFORM_ARCH
    )
    selected = manifest(name, chosen["digest"], token)
    selected_digest = chosen["digest"]
    document = json.loads(selected)

sys.stderr.write(f"index_digest={top_digest}\nmanifest_digest={selected_digest}\n")
index = {
    "schemaVersion": 2,
    "mediaType": "application/vnd.oci.image.index.v1+json",
    "manifests": [
        {
            "mediaType": document.get("mediaType", "application/vnd.docker.distribution.manifest.v2+json"),
            "digest": selected_digest,
            "size": len(selected),
            "annotations": {
                "io.containerd.image.name": f"docker.io/{name}:{tag}",
                "org.opencontainers.image.ref.name": tag,
            },
        }
    ],
}
with tarfile.open(fileobj=sys.stdout.buffer, mode="w|") as archive:
    add_bytes(archive, "oci-layout", b'{"imageLayoutVersion":"1.0.0"}')
    add_bytes(archive, "index.json", json.dumps(index).encode())
    add_bytes(archive, "blobs/sha256/" + selected_digest.split(":")[1], selected)
    add_blob(archive, name, token, document["config"]["digest"], document["config"]["size"])
    for layer in document["layers"]:
        add_blob(archive, name, token, layer["digest"], layer["size"])

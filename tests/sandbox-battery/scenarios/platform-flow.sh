set -eu
git_placeholder=$(printenv "$GIT_PH_NAME")
pkg_placeholder=$(printenv "$PKG_PH_NAME")
git_auth="http.extraHeader=Authorization: Bearer $git_placeholder"
owner=${REPO%%/*}
name=${REPO##*/}
stage_start=$(date +%s%N)

stage_done() {
  now=$(date +%s%N)
  echo "STAGE $1 $(((now - stage_start) / 1000000))"
  stage_start=$now
}

mkdir -p /work
cd /work
git -c "$git_auth" clone -q "https://$GIT_HOST/$REPO.git" "$BRANCH"
stage_done clone
cd "$BRANCH"
export "CARGO_REGISTRIES_${REG_UPPER}_INDEX=sparse+https://$PKG_HOST/api/packages/$owner/cargo/"
export "CARGO_REGISTRIES_${REG_UPPER}_TOKEN=Bearer $pkg_placeholder"
export CARGO_REGISTRY_GLOBAL_CREDENTIAL_PROVIDERS=cargo:token
export CARGO_HTTP_CAINFO=$SSL_CERT_FILE
cargo fetch > fetch.log 2>&1 || { tail -20 fetch.log; exit 20; }
stage_done packages
cargo build > build.log 2>&1 || { tail -20 build.log; exit 21; }
stage_done build
"./target/debug/$name" > run.log 2>&1 || { tail -5 run.log; exit 22; }
echo "RUN $(head -1 run.log)"
git checkout -q -b "$BRANCH"
echo "$BRANCH" > battery-marker.txt
git add battery-marker.txt
git -c user.name=battery -c user.email=battery@sbx.test commit -q -m "battery $BRANCH"
git -c "$git_auth" push -q origin "$BRANCH"
stage_done push

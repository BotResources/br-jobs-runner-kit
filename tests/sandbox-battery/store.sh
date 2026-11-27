#!/usr/bin/env bash
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/lib-cluster.sh"

store_env_file() {
  echo "$WORK/store-$MODE.env"
}

random_alnum() {
  openssl rand -hex "$((($1 + 1) / 2))" | cut -c1-"$1"
}

store_network() {
  if [ "$MODE" = closed ]; then
    echo "$CLOSED_NETWORK"
  else
    echo "k3d-$CLUSTER"
  fi
}

store_ip() {
  docker inspect "$STORE_CONTAINER" --format "{{(index .NetworkSettings.Networks \"$(store_network)\").IPAddress}}"
}

start_store_container() {
  if docker ps -a --format '{{.Names}}' | grep -qx "$STORE_CONTAINER"; then
    log "store container exists: $STORE_CONTAINER"
    docker start "$STORE_CONTAINER" >/dev/null
    return 0
  fi
  preflight
  local publish=()
  if [ "$MODE" = main ]; then
    publish=(-p "127.0.0.1:${STORE_HOST_PORT}:${STORE_PORT}")
  fi
  docker run -d --name "$STORE_CONTAINER" --network "$(store_network)" ${publish[@]+"${publish[@]}"} \
    "$SEAWEED_IMAGE" server -ip.bind=0.0.0.0 -s3 -dir=/data -volume.max=20 >/dev/null
}

weed_shell() {
  printf '%s\n' "$1" | docker exec -i "$STORE_CONTAINER" weed shell -master=localhost:9333 2>&1
}

wait_store_up() {
  for _ in $(seq 1 60); do
    if docker exec "$STORE_CONTAINER" sh -c "wget -q -S -O /dev/null http://127.0.0.1:${STORE_PORT}/ 2>&1 | grep -q HTTP/"; then
      return 0
    fi
    sleep 2
  done
  die "store not up"
}

configure_identity() {
  local ak sk file
  file="$(store_env_file)"
  if [ -s "$file" ]; then
    ak="$(grep '^S3_ACCESS_KEY=' "$file" | cut -d= -f2-)"
    sk="$(grep '^S3_SECRET_KEY=' "$file" | cut -d= -f2-)"
  else
    ak="SBX$(random_alnum 17)"
    sk="$(random_alnum 40)"
  fi
  weed_shell "s3.configure -user=sbx-battery -access_key=$ak -secret_key=$sk -actions=Admin -apply" >/dev/null
  umask 077
  {
    echo "S3_ACCESS_KEY=$ak"
    echo "S3_SECRET_KEY=$sk"
    echo "S3_BUCKET=$BUCKET"
    echo "S3_REGION=us-east-1"
    echo "S3_PATH_STYLE=true"
    echo "S3_ENDPOINT_IN_CLUSTER=http://$STORE_SERVICE.$NS_STORE.svc.cluster.local:$STORE_PORT"
  } >"$file"
  if [ "$MODE" = main ]; then
    echo "S3_ENDPOINT_HOST=http://127.0.0.1:$STORE_HOST_PORT" >>"$file"
  fi
}

create_bucket() {
  if weed_shell "s3.bucket.list" | grep -qw "$BUCKET"; then
    log "bucket exists: $BUCKET"
  else
    weed_shell "s3.bucket.create -name $BUCKET" | tail -1 >&2
  fi
  if [ -n "${SAVES_TTL:-}" ]; then
    weed_shell "fs.configure -locationPrefix=/buckets/$BUCKET/$SAVES_PREFIX -ttl=$SAVES_TTL -apply" >/dev/null
  fi
}

publish_store_service() {
  local ip
  ip="$(store_ip)"
  kc apply -f - >&2 <<YAML
apiVersion: v1
kind: Service
metadata:
  name: $STORE_SERVICE
  namespace: $NS_STORE
spec:
  ports:
    - {name: s3, port: $STORE_PORT, targetPort: $STORE_PORT}
---
apiVersion: v1
kind: Endpoints
metadata:
  name: $STORE_SERVICE
  namespace: $NS_STORE
subsets:
  - addresses:
      - ip: $ip
    ports:
      - {name: s3, port: $STORE_PORT}
YAML
}

setup_store() {
  start_store_container
  wait_store_up
  configure_identity
  create_bucket
  publish_store_service
}

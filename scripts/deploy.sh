#!/usr/bin/env bash
set -eu
TARGET="carbon.hacklab"
TARGET_DIR="/srv/discord-mail-bridge"

mkdir -p $(pwd)/target_docker
docker run -it --rm -v "$(pwd):/src" -v "$(pwd)/target_docker:/src/target" -w /src rust:1-bullseye cargo build --release
scp target_docker/release/discord-mail-bridge $TARGET:$TARGET_DIR/
scp contrib/discord-mail-bridge.service $TARGET:$TARGET_DIR/
scp contrib/discord-mail-bridge.timer $TARGET:$TARGET_DIR/
ssh $TARGET sudo install -m644 $TARGET_DIR/discord-mail-bridge.service /etc/systemd/system/discord-mail-bridge.service
ssh $TARGET sudo install -m644 $TARGET_DIR/discord-mail-bridge.timer /etc/systemd/system/discord-mail-bridge.timer
ssh -tt carbon.hacklab sudo systemctl daemon-reload
ssh -tt carbon.hacklab sudo systemctl enable --now discord-mail-bridge.timer

#! /bin/bash

echo "sample build start..."

cd "sample"

yarn install && yarn build

echo "build docker image..."

docker build -f ./Dockerfile -t node-vsock-sample-server

echo "sample build completed."

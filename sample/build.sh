#! /bin/bash

echo "build and pack node-vsock..."
cd ..
yarn build:all
yarn pack
mkdir -p sample/tmp/
mv *.tgz sample/tmp/node-vsock.tgz
mv *.node sample/tmp/

if [ "$1" == "preinstall" ]; then
  exit 0
fi

echo "build sample..."
cd "sample"
yarn install --ignore-scripts && yarn build

if [ "$1" != "all" ]; then
  exit 0
fi

echo "build sample server docker image..."
docker build -f ./Dockerfile.server -t node-vsock-sample-server

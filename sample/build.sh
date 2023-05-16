#! /bin/bash

echo "build and pack node-vsock..."
cd ..
yarn build:debug --target=x86_64-unknown-linux-musl
yarn build:debug --target=x86_64-unknown-linux-gnu
yarn build:ts
yarn pack

mkdir -p sample/tmp/
mv -f *.tgz sample/tmp/node-vsock.tgz
cp -f *.node sample/tmp/

if [ "$1" == "preinstall" ]; then
  exit 0
fi

echo "build sample..."
cd "sample"
yarn install --ignore-scripts && yarn build
cp -f tmp/node-vsock.linux-x64-gnu.node node_modules/node-vsock/

if [ "$1" != "all" ]; then
  exit 0
fi

echo "build sample server docker image..."
docker build -f ./Dockerfile.server -t node-vsock-sample-server .

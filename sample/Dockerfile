# SPDX-License-Identifier: Apache-2.0

FROM node:16-alpine

RUN apk update && apk add --no-cache bash gcompat
RUN mkdir -p /app

WORKDIR /app

COPY addon.js .
COPY node-vsock.linux-x64-musl.node .
COPY dist/index.js .

RUN mkdir -p /app/sample

COPY dist/sample/server.js /app/sample/

COPY sample/package.json .
COPY sample/run.sh .

RUN chmod a+x /app/run.sh

CMD ["/app/run.sh"]
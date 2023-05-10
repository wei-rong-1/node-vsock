import { VsockSocket } from "../index"

const client = new VsockSocket();

client.connect(100, 9001, () => {
  const data = ['hello, ', 'w', 'o', 'r', 'l', 'd'];

  client.on('data', (buf: Buffer) => {
    console.log("recv: ", buf.toString())
  })

  for (const str of data) {
    client.writeSync(Buffer.from(str));
  }
});
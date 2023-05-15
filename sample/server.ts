import { VsockServer, VsockSocket } from 'node-vsock'

// function sleep(s: number) {
//   return new Promise((resolve) => {
//     setTimeout(resolve, s * 1000);
//   });
// }

console.log("sample server start..")

async function main() {
  const server = new VsockServer();

  server.on('error', (err: Error) => {
    console.log("err: ", err)
  });

  server.on('connection', (socket: VsockSocket) => {
    console.log("new socket connection..")

    socket.on('error', (err) => {
      console.log("socket err: ", err)
    });

    socket.on('data', (buf: Buffer) => {
      const content = buf.toString()
      console.log('socket recv: ', content)
      socket.writeTextSync(`I hear you! ${content}`)
    });

    // setTimeout(() => {
    //   socket.end()
    // }, 10 * 1000)
  });

  server.listen(9001);

  console.log("sample server listening..")
}

main()
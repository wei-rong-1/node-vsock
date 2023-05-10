import { VsockServer, VsockSocket } from '../index'

const server = new VsockServer();

server.listen(9001);

server.on('connection', (socket: VsockSocket) => {
  socket.on('data', (buf: Buffer) => {
    const content = buf.toString();

    console.log('recv: ', content);

    socket.writeTextSync(`hear you! ${content}`)
  });
});

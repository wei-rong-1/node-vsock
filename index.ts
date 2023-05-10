import { EventEmitter } from 'events';
import { VsockSocket as VsockSocketAddon } from './addon';

export type Callback = () => void;

export class VsockServer extends EventEmitter {
  private closed: boolean = false;
  private socket: VsockSocketAddon;

  constructor() {
    super();

    this.emit = this.emit.bind(this);
    this.socket = new VsockSocketAddon(this.emit);

    this.on('_connection', this.onConnection);
    this.on('_error', this.onError);
  }

  private onError = (err: Error) => {
    // TODO test this
    this.emit('error', err);
  };

  private onConnection = (fd: number) => {
    const socket = new VsockSocket(fd);
    this.emit('connection', socket);
  };

  close() {
    if (this.closed) {
      return;
    }

    this.closed = true;
    this.socket.close();
  }

  listen(port: number) {
    if (this.closed) {
      throw new Error('Socket has been closed');
    }

    // TODO may be error
    this.socket.listen(port);
  }
}

export class VsockSocket extends EventEmitter {
  private socket: VsockSocketAddon;
  private destroyed: boolean = false;
  private connectCallback?: Callback;

  constructor(fd?: number) {
    super();

    this.emit = this.emit.bind(this);
    this.socket = new VsockSocketAddon(this.emit, fd);

    // TODO may be error
    if (fd) {
      this.socket.startRecv();
    }

    this.on('_data', this.onData);
    this.on('_connect', this.onConnect);
    this.on('_error', this.onError);
  }

  private onData = (buf: Buffer) => {
    this.emit('data', buf);
  };

  private checkDestroyed() {
    if (this.destroyed) {
      throw new Error('Socket has been destroyed');
    }
  }

  private onError = (err: Error) => {
    process.nextTick(() => {
      this.emit('error', err);
      this.destroy();
    });
  };

  private onConnect = () => {
    this.emit('connect');
    this.socket.startRecv();

    if (this.connectCallback) {
      this.connectCallback();
    }
  };

  connect(cid: number, port: number, connectCallback?: Callback) {
    this.checkDestroyed();
    this.connectCallback = connectCallback;

    this.socket.connect(cid, port);
  }

  writeSync(buf: Buffer) {
    this.checkDestroyed();

    this.socket.writeBuffer(buf);
  }

  writeTextSync(data: string) {
    this.checkDestroyed();

    this.socket.writeText(data);
  }

  destroy() {
    if (this.destroyed) {
      return;
    }

    this.destroyed = true;
    this.socket.close();
  }
}
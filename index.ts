import { EventEmitter } from 'events';
import { VsockSocket as VsockSocketAddon } from './addon';

export type Callback = () => void;

export class VsockServer extends EventEmitter {
  private closed: boolean = false;
  private socket: VsockSocketAddon;

  constructor() {
    super();

    const emit = this.emit = this.emit.bind(this);
    this.socket = new VsockSocketAddon(function (err: Error, eventName: string, ...args) {
      console.log("server event: ", err, eventName, args)
      emit(eventName, ...args);
    });

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
  private connectCallback?: Callback;
  private shutdownCallback?: Callback;
  private isDestroyed: boolean = false;
  private isShutdown: boolean = false;
  private isEnd: boolean = false

  constructor(fd?: number) {
    super();

    const emit = this.emit = this.emit.bind(this);
    this.socket = new VsockSocketAddon(function (err: Error, eventName: string, ...args) {
      console.log("socket event: ", err, eventName, args)
      emit(eventName, ...args);
    }, fd);

    // TODO may be error
    if (fd) {
      this.socket.startRecv();
    }

    this.on('_data', this.onData);
    this.on('_connect', this.onConnect);
    this.on('_error', this.onError);
    this.on('_shutdown', this.onShutdown);
    this.on('end', this.onEnd);
  }

  private checkDestroyed() {
    if (this.isDestroyed) {
      throw new Error('Socket has been destroyed');
    }
  }

  private tryClose() {
    if (this.isEnd && this.isShutdown) {
      this.destroy()
    }
  }

  private onData = (buf: Buffer) => {
    this.emit('data', buf);
  };

  private onError = (err: Error) => {
    process.nextTick(() => {
      // unhandled error emitted from emitter will cause stopping process.
      // incoming socket have to listen on 'error' event to bypass this problem now.
      this.emit('error', err);
    });
  };

  private onEnd = () => {
    this.isEnd = true;
    this.tryClose();
  }

  private onShutdown = () => {
    this.isShutdown = true;
    this.tryClose();

    if (this.shutdownCallback) {
      this.shutdownCallback();
    }
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

  end(callback?: Callback) {
    this.shutdownCallback = callback;
    this.socket.end();
  }

  destroy() {
    if (this.isDestroyed) {
      return;
    }

    this.isDestroyed = true;
    this.socket.close();
  }
}
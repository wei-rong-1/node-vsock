import { VsockSocket } from "../index"

function sleep(s:number) {
  return new Promise((resolve) => {
    setTimeout(resolve, s*1000);
  });
}

async function main() {
  console.log("start sample client..")

  const client = new VsockSocket();
  
  client.on('error', (err:Error) => {
    console.log("err: ", err)
  });

  client.connect(15, 9001, async () => {
    const data = ['hello, ', 'w', 'o', 'r', 'l', 'd'];

    client.on('data', (buf: Buffer) => {
      console.log("recv: ", buf.toString())
    })
  
    for (const str of data) {
      console.log("send: ", str)
      client.writeTextSync(str);
      await sleep(3);
    }
  
    // await sleep(20);
    client.end();
  });

  console.log("end sample client.")
}

main()
import test from 'ava'

// import { plus100 } from '../addon'
function plus100(n:number) {
    return n + 100
}

test('sync function from native code', (t) => {
  const fixture = 42
  t.is(plus100(fixture), fixture + 100)
})

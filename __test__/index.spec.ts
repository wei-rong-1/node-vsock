import test from 'ava'

import { VsockSocket } from '../index'

test('sync function from native code', (t) => {
	t.is(!!VsockSocket, true);
})

import { mount } from 'svelte';

import '../../src/styles/app.css';
import '../../src/lib/editor/editor.css';

import EditorSpike from './EditorSpike.svelte';

const target = document.getElementById('app');
if (!target) throw new Error('index.html has no #app element');

export default mount(EditorSpike, { target });

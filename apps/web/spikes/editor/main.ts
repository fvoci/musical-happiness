import { mount } from 'svelte';

import './spike.css';
import '../../src/lib/editor/editor.css';

import { loadFontFaces } from '../../src/lib/fonts.ts';

import EditorSpike from './EditorSpike.svelte';

const target = document.getElementById('app');
if (!target) throw new Error('index.html has no #app element');

// The spike is also where a person tries typing Korean: draw it with the app's fonts.
loadFontFaces(document.documentElement.lang || navigator.language);

export default mount(EditorSpike, { target });

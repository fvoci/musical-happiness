import { mount } from 'svelte';

import './drag.css';
import { watchSystemTheme } from '$lib/theme.svelte.js';

import Board from './Board.svelte';

const target = document.getElementById('app');
if (!target) throw new Error('index.html has no #app element');

watchSystemTheme();

export default mount(Board, { target });

import './style.css';
import { startApp } from './app';
import { initEngine } from './engine';

await initEngine();
await startApp();

import * as React from 'react';
import { createRoot } from 'react-dom/client';
import { BrowserRouter } from 'react-router-dom';

import { App } from './app/App';

import './app/globals.css';
import './app/animations.css';

/**
 * Desktop entry point.
 *
 * The provider stack still sits *above* the router outlet — that is what keeps
 * audio playing while a route swap happens underneath the player.
 */
const container = document.getElementById('root');
if (!container) throw new Error('VoxLeaf could not find its root element.');

createRoot(container).render(
  <React.StrictMode>
    <BrowserRouter>
      <App />
    </BrowserRouter>
  </React.StrictMode>,
);

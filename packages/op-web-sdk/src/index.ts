// Public entry for the OpenPencil read-only web SDK core.
export const VERSION = '0.10.1';
export { createViewer, OpViewer } from './viewer.js';
export type { Viewport, CreateViewerOptions, PenDocument, PenPage } from './types.js';
export type { ViewerEvent } from './events.js';

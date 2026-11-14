import 'dotenv/config';
import path from 'node:path';
import express from 'express';
import { createServer as createViteServer } from 'vite';
import { securityHeadersMiddleware } from './server/middleware';
import { createChatRouter } from './server/routes/chatRouter';
import { createControlPlaneRouter } from './server/routes/controlPlaneRouter';
import { createSessionsRouter } from './server/routes/sessionsRouter';
import { AtomicStateStore } from './server/store';

async function startServer() {
  const store = new AtomicStateStore();
  const app = express();

  app.use(express.json({ limit: '2mb' }));
  app.use(securityHeadersMiddleware);

  // Domain API routers
  app.use('/api', createSessionsRouter(store));
  app.use('/api', createChatRouter(store));
  app.use('/api', createControlPlaneRouter(store));

  // Vite integration in development, static dist in production
  if (process.env.NODE_ENV !== 'production') {
    const vite = await createViteServer({
      server: { middlewareMode: true },
      appType: 'spa',
    });
    app.use(vite.middlewares);
  } else {
    const distDir = path.resolve(process.cwd(), 'dist');
    app.use(express.static(distDir));
    app.get('*', (_req, res) => {
      res.sendFile(path.join(distDir, 'index.html'));
    });
  }

  const PORT = Number(process.env.PORT) || 3000;
  const server = app.listen(PORT, '0.0.0.0', () => {
    console.log(`PurrCode AI Cockpit server listening on http://0.0.0.0:${PORT}`);
  });

  const shutdown = () => {
    store.persist();
    server.close(() => process.exit(0));
  };
  process.once('SIGINT', shutdown);
  process.once('SIGTERM', shutdown);
}

startServer();

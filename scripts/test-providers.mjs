import 'dotenv/config';

const OR_KEY = process.env.OPENROUTER_API_KEY;
const OR_MODEL = process.env.OPENROUTER_MODEL || 'openai/gpt-oss-20b:free';
const DG_KEY = process.env.DEEPGRAM_API_KEY;

function timeout(ms) {
  return new Promise((_, rej) => setTimeout(() => rej(new Error('timeout')), ms));
}

async function testOpenRouter() {
  if (!OR_KEY) throw new Error('Missing OPENROUTER_API_KEY');
  const ctrl = new AbortController();
  const t = setTimeout(()=> ctrl.abort(), 4000);
  try {
    const res = await fetch('https://openrouter.ai/api/v1/chat/completions', {
      method: 'POST',
      headers: { 'content-type': 'application/json', 'authorization': `Bearer ${OR_KEY}` },
      body: JSON.stringify({
        model: OR_MODEL,
        messages: [
          { role: 'system', content: 'Return plain text.' },
          { role: 'user', content: 'hello' }
        ]
      }),
      signal: ctrl.signal
    });
    if (!res.ok) throw new Error(`OpenRouter HTTP ${res.status}`);
    const j = await res.json();
    const text = j?.choices?.[0]?.message?.content?.trim?.();
    if (!text) throw new Error('Empty content');
  } finally { clearTimeout(t); }
}

async function testDeepgram() {
  if (!DG_KEY) throw new Error('Missing DEEPGRAM_API_KEY');
  const { WebSocket } = await import('ws');
  await new Promise((resolve, reject) => {
    const ws = new WebSocket('wss://api.deepgram.com/v1/listen', { headers: { Authorization: `Token ${DG_KEY}` } });
    const to = setTimeout(()=> { try { ws.terminate(); } catch {} reject(new Error('Deepgram timeout')); }, 3000);
    ws.on('open', () => { clearTimeout(to); try { ws.close(); } catch {} resolve(); });
    ws.on('error', (e) => { clearTimeout(to); reject(e); });
  });
}

try {
  await testOpenRouter();
  console.log('OpenRouter OK');
  await testDeepgram();
  console.log('Deepgram OK');
  process.exit(0);
} catch (e) {
  console.error('Provider test failed:', e?.message || e);
  process.exit(1);
}


import { useEffect, useRef } from 'react';

export function Waveform({ analyser }: { analyser: AnalyserNode }) {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const rafRef = useRef(0);

  useEffect(() => {
    const canvas = canvasRef.current!;
    const ctx = canvas.getContext('2d')!;
    const bufferLength = analyser.fftSize;
    const dataArray = new Uint8Array(bufferLength);

    // Keep a smoothed audio energy value so the bars feel lively even on quiet input
    let smoothedLevel = 0;

    const draw = () => {
      rafRef.current = requestAnimationFrame(draw);

      // Use time‑domain data to capture overall loudness (more responsive to quiet speech)
      analyser.getByteTimeDomainData(dataArray);
      let sum = 0;
      for (let i = 0; i < bufferLength; i++) {
        const centered = dataArray[i] - 128;
        sum += centered * centered;
      }
      const rms = Math.sqrt(sum / bufferLength) / 128; // 0..1

      // Boost quiet input aggressively and clamp to 1 (works even when echo/NS are off)
      const boosted = Math.min(1, rms * 5 + 0.08); // +floor keeps idle motion to show it's live
      smoothedLevel = smoothedLevel * 0.55 + boosted * 0.45; // fast-ish response without flicker

      const w = (canvas.width = canvas.offsetWidth * devicePixelRatio);
      const h = (canvas.height = canvas.offsetHeight * devicePixelRatio);
      ctx.setTransform(devicePixelRatio, 0, 0, devicePixelRatio, 0, 0);
      ctx.clearRect(0, 0, w, h);

      const bars = 10; // Reduced for minimal pill design
      const barWidth = 2.5; // Fixed width for consistency
      const gap = 3; // Gap between bars
      const totalWidth = bars * barWidth + (bars - 1) * gap;
      const startX = (canvas.offsetWidth - totalWidth) / 2; // Center the bars
      const maxHeight = canvas.offsetHeight * 0.9; // Allow taller bars for visibility

      for (let i = 0; i < bars; i++) {
        // Slight center bias so outer bars are a bit shorter (pleasant taper)
        const centerBias = 1 - Math.abs((i - (bars - 1) / 2) / (bars / 2)) * 0.35;
        const height = Math.max(4, smoothedLevel * maxHeight * centerBias);
        const x = startX + i * (barWidth + gap);
        const y = (canvas.offsetHeight - height) / 2;
        ctx.fillStyle = 'rgba(242,241,234,0.82)';
        ctx.beginPath();
        ctx.roundRect(x, y, barWidth, height, 2); // Rounded ends
        ctx.fill();
      }
    };
    rafRef.current = requestAnimationFrame(draw);
    return () => cancelAnimationFrame(rafRef.current);
  }, [analyser]);

  return <canvas ref={canvasRef} className="w-full h-full" />;
}


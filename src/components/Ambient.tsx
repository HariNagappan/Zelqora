// Animated futuristic environment (design §2).
//
// Canvas-based so the moving particles never re-render the React tree: the
// component subscribes to the effects object through a mutable ref and only
// mounts/unmounts the animation loop. It pauses when the document is hidden,
// respects `prefers-reduced-motion` (drawing a single static frame), and
// honours the per-effect switches and animation intensity.

import { useEffect, useRef } from "react";

import type { Effects, Intensity } from "../types";
import { ambientSpeed } from "../theme/themes";

interface Particle {
  x: number;
  y: number;
  vx: number;
  vy: number;
  z: number;
  a: number;
}

interface Node {
  x: number;
  y: number;
  vx: number;
  vy: number;
}

interface Edge {
  a: number;
  b: number;
}

interface Pulse {
  e: number;
  p: number;
  s: number;
}

const PARTICLES = 64;
const NODES = 24;

function hexToRgbString(hex: string): string {
  const value = Number.parseInt(hex.replace("#", ""), 16);
  if (Number.isNaN(value)) {
    return "141,238,255";
  }
  return [(value >> 16) & 255, (value >> 8) & 255, value & 255].join(",");
}

function readVar(element: HTMLElement, name: string, fallback: string): string {
  const value = getComputedStyle(element).getPropertyValue(name).trim();
  if (!value) {
    return fallback;
  }
  if (value.startsWith("#")) {
    return hexToRgbString(value);
  }
  const match = value.match(/rgba?\(([^)]+)\)/);
  if (match) {
    const parts = match[1].split(",").map((part) => part.trim());
    return `${parts[0]},${parts[1]},${parts[2]}`;
  }
  return fallback;
}

export default function Ambient({
  effects,
  voiceActive,
  successFlash,
}: {
  effects: Effects;
  /** True while Nova is listening — the environment becomes more energetic. */
  voiceActive: boolean;
  /** Bumped when a command succeeds, for the short confirmation pulse. */
  successFlash: number;
}) {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const fxRef = useRef({
    particles: effects.particles,
    network: effects.network,
    parallax: effects.parallax,
    voice: effects.voice,
    speed: ambientSpeed(effects.intensity),
  });
  const voiceRef = useRef(voiceActive);
  const flashRef = useRef(0);
  const colorsRef = useRef({ fx1: "141,238,255", fx2: "167,139,255" });

  // The effects object is pushed into a ref, never React state: changing
  // appearance must not restart (or re-render) anything.
  useEffect(() => {
    fxRef.current = {
      particles: effects.particles,
      network: effects.network,
      parallax: effects.parallax,
      voice: effects.voice,
      speed: ambientSpeed(effects.intensity as Intensity),
    };
    if (canvasRef.current) {
      colorsRef.current = {
        fx1: readVar(document.body, "--fx1", "141,238,255"),
        fx2: readVar(document.body, "--fx2", "167,139,255"),
      };
    }
  }, [effects]);

  useEffect(() => {
    voiceRef.current = voiceActive;
  }, [voiceActive]);

  useEffect(() => {
    if (successFlash > 0) {
      flashRef.current = 1;
    }
  }, [successFlash]);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) {
      return;
    }
    const context = canvas.getContext("2d");
    if (!context) {
      return;
    }

    const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    const dpr = Math.min(window.devicePixelRatio || 1, 1.5);
    let width = 0;
    let height = 0;
    let particles: Particle[] = [];
    let nodes: Node[] = [];
    let edges: Edge[] = [];
    let pulses: Pulse[] = [];
    let level = 1;
    let target = 1;
    let time = 0;
    let frame = 0;

    const seed = () => {
      width = canvas.clientWidth || window.innerWidth;
      height = canvas.clientHeight || window.innerHeight;
      canvas.width = Math.max(1, Math.floor(width * dpr));
      canvas.height = Math.max(1, Math.floor(height * dpr));
      context.setTransform(dpr, 0, 0, dpr, 0, 0);

      particles = Array.from({ length: PARTICLES }, () => ({
        x: Math.random() * width,
        y: Math.random() * height,
        vx: (Math.random() - 0.5) * 0.16,
        vy: (Math.random() - 0.5) * 0.16,
        z: 0.35 + Math.random() * 0.65,
        a: Math.random() * 0.5 + 0.12,
      }));
      nodes = Array.from({ length: NODES }, () => ({
        x: Math.random() * width,
        y: Math.random() * height,
        vx: (Math.random() - 0.5) * 0.1,
        vy: (Math.random() - 0.5) * 0.1,
      }));
      edges = [];
      nodes.forEach((_, index) => {
        for (let k = 0; k < 3; k += 1) {
          const target2 = Math.floor(Math.random() * nodes.length);
          if (target2 !== index && !edges.some((edge) => edge.a === target2 && edge.b === index)) {
            edges.push({ a: index, b: target2 });
          }
        }
      });
      pulses = Array.from({ length: 8 }, () => ({
        e: Math.floor(Math.random() * Math.max(1, edges.length)),
        p: Math.random(),
        s: 0.0016 + Math.random() * 0.003,
      }));
      colorsRef.current = {
        fx1: readVar(document.body, "--fx1", "141,238,255"),
        fx2: readVar(document.body, "--fx2", "167,139,255"),
      };
    };

    const draw = (timestamp: number) => {
      const fx = fxRef.current;
      const { fx1, fx2 } = colorsRef.current;
      time = timestamp || time + 16;

      if (!document.hidden) {
        // The environment reacts to Nova's state: idle → listening → processing.
        const energy = voiceRef.current && fx.voice ? 1.65 : 1.15;
        target = energy * fx.speed;
        level += (target - level) * 0.025;
        context.clearRect(0, 0, width, height);
        context.lineWidth = 1;

        // Perspective grid.
        if (fx.network) {
          const offset = (time * 0.004 * level) % 64;
          context.strokeStyle = `rgba(${fx2},.045)`;
          for (let gx = -64 + offset; gx < width + 64; gx += 64) {
            context.beginPath();
            context.moveTo(gx, 0);
            context.lineTo(gx - 90, height);
            context.stroke();
          }
          for (let gy = 24; gy < height; gy += 56) {
            context.beginPath();
            context.moveTo(0, gy);
            context.lineTo(width, gy);
            context.stroke();
          }
        }

        // Drifting particles.
        if (fx.particles) {
          for (const particle of particles) {
            const depth = fx.parallax ? particle.z : 1;
            particle.x += particle.vx * level;
            particle.y += particle.vy * level;
            if (particle.x < 0) particle.x = width;
            if (particle.x > width) particle.x = 0;
            if (particle.y < 0) particle.y = height;
            if (particle.y > height) particle.y = 0;
            context.fillStyle = `rgba(${fx1},${(particle.a * 0.55 * depth).toFixed(3)})`;
            context.beginPath();
            context.arc(particle.x, particle.y, 1.4 * depth, 0, Math.PI * 2);
            context.fill();
          }
        }

        // Neural network + travelling light pulses.
        if (fx.network) {
          for (const node of nodes) {
            node.x += node.vx * level;
            node.y += node.vy * level;
            if (node.x < 0 || node.x > width) node.vx *= -1;
            if (node.y < 0 || node.y > height) node.vy *= -1;
          }
          for (const edge of edges) {
            const a = nodes[edge.a];
            const b = nodes[edge.b];
            if (!a || !b) continue;
            const distance = Math.hypot(a.x - b.x, a.y - b.y);
            if (distance > 190) continue;
            context.strokeStyle = `rgba(${fx2},${Math.max(0.02, (0.15 - distance / 1500) * Math.min(1.4, level)).toFixed(3)})`;
            context.beginPath();
            context.moveTo(a.x, a.y);
            context.lineTo(b.x, b.y);
            context.stroke();
          }
          for (const pulse of pulses) {
            const edge = edges[pulse.e];
            if (!edge) continue;
            const a = nodes[edge.a];
            const b = nodes[edge.b];
            if (!a || !b) continue;
            pulse.p += pulse.s * level;
            if (pulse.p > 1) {
              pulse.p = 0;
              pulse.e = Math.floor(Math.random() * Math.max(1, edges.length));
            }
            context.fillStyle = `rgba(${fx1},${(0.45 * Math.min(1.2, level)).toFixed(3)})`;
            context.beginPath();
            context.arc(a.x + (b.x - a.x) * pulse.p, a.y + (b.y - a.y) * pulse.p, 1.5, 0, Math.PI * 2);
            context.fill();
          }
        }

        // Short confirmation pulse after a successful command.
        if (flashRef.current > 0) {
          flashRef.current -= 0.02;
          const gradient = context.createRadialGradient(
            width * 0.5,
            height * 0.5,
            0,
            width * 0.5,
            height * 0.5,
            Math.max(width, height) * 0.6,
          );
          gradient.addColorStop(0, `rgba(${fx1},${(0.1 * flashRef.current).toFixed(3)})`);
          gradient.addColorStop(1, `rgba(${fx1},0)`);
          context.fillStyle = gradient;
          context.fillRect(0, 0, width, height);
        }
      }

      if (!reducedMotion) {
        frame = window.requestAnimationFrame(draw);
      }
    };

    seed();
    const resizeObserver = new ResizeObserver(() => seed());
    resizeObserver.observe(canvas);

    if (reducedMotion) {
      // One static frame — the environment still exists, it just doesn't move.
      draw(16);
    } else {
      frame = window.requestAnimationFrame(draw);
    }

    const onVisibility = () => {
      if (document.hidden) {
        window.cancelAnimationFrame(frame);
      } else if (!reducedMotion) {
        frame = window.requestAnimationFrame(draw);
      }
    };
    document.addEventListener("visibilitychange", onVisibility);

    return () => {
      window.cancelAnimationFrame(frame);
      resizeObserver.disconnect();
      document.removeEventListener("visibilitychange", onVisibility);
    };
  }, []);

  return <canvas id="bgl" ref={canvasRef} aria-hidden="true" />;
}

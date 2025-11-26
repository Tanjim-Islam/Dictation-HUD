import { motion } from 'framer-motion';

export function Badge({ text }: { text: string }) {
  return (
    <motion.div className="px-3 py-1 rounded-full border shadow-sm"
      style={{
        borderColor: 'var(--badge-border, #7a1f1f)',
        background: 'var(--badge-bg, #fbe9e9)',
        color: 'var(--badge-text, #7a1f1f)'
      }}>
      <span className="text-xs">{text}</span>
    </motion.div>
  );
}


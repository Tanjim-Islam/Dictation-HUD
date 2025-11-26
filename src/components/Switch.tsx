type Props = { checked: boolean; onCheckedChange?: (v: boolean)=>void; ariaLabel?: string };
export function Switch({ checked, onCheckedChange, ariaLabel }: Props) {
  return (
    <button
      role="switch"
      aria-checked={checked}
      aria-label={ariaLabel}
      onClick={()=> onCheckedChange?.(!checked)}
      className={`w-11 h-6 rounded-full transition-colors ${checked? 'bg-accent' : 'bg-neutral-700'} relative`}
    >
      <span className={`absolute top-0.5 ${checked? 'left-6' : 'left-0.5'} w-5 h-5 rounded-full bg-black transition-all`} />
    </button>
  );
}

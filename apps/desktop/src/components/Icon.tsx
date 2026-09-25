import { icons, type LucideProps } from 'lucide-react';

function pascal(name: string): string {
  return name
    .split('-')
    .map((part) => part.charAt(0).toUpperCase() + part.slice(1))
    .join('');
}

/** A Lucide icon by its kebab-case name (as modules declare them), drawn square like the design. */
export function Icon({
  name,
  size = 14,
  className = 'i',
  ...rest
}: { name: string } & LucideProps) {
  const Svg = icons[pascal(name) as keyof typeof icons] ?? icons.Box;
  return (
    <Svg
      size={size}
      strokeWidth={1.5}
      strokeLinecap="square"
      strokeLinejoin="miter"
      className={className}
      aria-hidden="true"
      {...rest}
    />
  );
}

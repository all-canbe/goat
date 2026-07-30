interface GoatLogoProps {
  size?: number;
  className?: string;
}

export default function GoatLogo({ size = 20, className = "" }: GoatLogoProps) {
  return (
    <div
      style={{ width: size, height: size }}
      className={`relative flex items-center justify-center shrink-0 rounded-lg bg-gradient-to-br from-indigo-500 via-indigo-600 to-cyan-500 p-0.5 shadow-sm shadow-indigo-500/20 ${className}`}
    >
      <svg
        viewBox="0 0 24 24"
        fill="none"
        xmlns="http://www.w3.org/2000/svg"
        className="w-full h-full text-white"
      >
        <defs>
          <linearGradient id="goatGrad" x1="2" y1="2" x2="22" y2="22" gradientUnits="userSpaceOnUse">
            <stop stopColor="#818CF8" />
            <stop offset="0.5" stopColor="#6366F1" />
            <stop offset="1" stopColor="#22D3EE" />
          </linearGradient>
        </defs>
        {/* Sleek Geometric Goat Horns and Head */}
        <path
          d="M6 3C5 5.5 5.5 8.5 7 10.5M18 3C19 5.5 18.5 8.5 17 10.5"
          stroke="currentColor"
          strokeWidth="2"
          strokeLinecap="round"
        />
        <path
          d="M12 4L7 9.5L9.5 17.5L12 21L14.5 17.5L17 9.5L12 4Z"
          fill="url(#goatGrad)"
          stroke="currentColor"
          strokeWidth="1.2"
          strokeLinejoin="round"
        />
        <circle cx="10" cy="11" r="1" fill="#FFFFFF" />
        <circle cx="14" cy="11" r="1" fill="#FFFFFF" />
      </svg>
    </div>
  );
}

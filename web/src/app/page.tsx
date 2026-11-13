import Link from "next/link";

export default function Title() {
  return (
    <main className="flex min-h-screen flex-col items-center justify-center gap-6 px-4 text-center">
      <h1 className="font-mono text-5xl font-black tracking-[0.2em] text-[#ff3db8] sm:text-7xl">VOIDWRAP RUN</h1>
      <p className="font-mono text-lg text-[#38e8ff]">Collect every gift. Outrun what you voted for.</p>
      <Link
        href="/play"
        data-testid="play"
        className="rounded border-2 border-[#ff3db8] px-8 py-3 font-mono text-xl font-bold uppercase tracking-widest hover:bg-[#ff3db8] hover:text-black"
      >
        Play
      </Link>
    </main>
  );
}

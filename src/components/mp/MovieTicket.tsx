import { motion } from "framer-motion";
import logoMark from "../../assets/logo_mark.png";

type MovieTicketProps = {
  code?: string;
};

export function MovieTicket({ code = "— — — — — —" }: MovieTicketProps) {
  return (
    <motion.div
      initial={{ opacity: 0, y: 30, rotate: -6 }}
      animate={{ opacity: 1, y: [0, -6, 0], rotate: [-4, -2, -4] }}
      transition={{
        opacity: { duration: 0.9, ease: "easeOut" },
        y: { duration: 7, repeat: Infinity, ease: "easeInOut" },
        rotate: { duration: 9, repeat: Infinity, ease: "easeInOut" },
      }}
      className="relative w-[320px] h-[480px]"
      data-testid="movie-ticket"
    >
      <div
        className="absolute -inset-10 pointer-events-none"
        style={{
          background:
            "radial-gradient(ellipse at center, rgba(159,122,234,0.35) 0%, transparent 65%)",
          filter: "blur(30px)",
        }}
      />

      <div
        className="relative w-full h-full rounded-2xl overflow-hidden"
        style={{
          background: "linear-gradient(160deg, #1A1230 0%, #0C0818 55%, #12082A 100%)",
          border: "1px solid rgba(159,122,234,0.35)",
          boxShadow:
            "0 30px 80px -20px rgba(107,70,193,0.55), inset 0 1px 0 rgba(255,255,255,0.06)",
        }}
      >
        <div className="ticket-notch left" style={{ top: "62%" }} />
        <div className="ticket-notch right" style={{ top: "62%" }} />

        <div className="relative h-[62%] p-6 flex flex-col">
            <div className="flex items-center justify-between text-white/60">
              <div className="flex items-center gap-2">
                <img src={logoMark} alt="" className="w-5 h-5 rounded-full object-cover" />
                <span className="text-[10px] tracking-[0.28em] uppercase">Movie Party</span>
              </div>
              <span className="text-[10px] tracking-[0.28em] uppercase">Admit One</span>
            </div>

          <div className="flex-1 flex flex-col items-start justify-end pb-1">
            <span className="text-[10px] tracking-[0.28em] uppercase text-white/50 mb-3">
              Tonight's Feature
            </span>
            <h3 className="font-serif-display text-[42px] leading-[0.95] text-white">
              A Private
              <br />
              Cinema
            </h3>
            <p className="text-white/60 text-xs mt-4 tracking-wide">Row A · Seat 2 · Reserved</p>
          </div>

          <div
            className="absolute top-6 right-6 w-16 h-16 rounded-full"
            style={{
              background:
                "radial-gradient(circle at center, rgba(159,122,234,0.5) 0%, transparent 70%)",
              filter: "blur(6px)",
            }}
          />
        </div>

        <div className="absolute left-4 right-4 top-[62%] ticket-dashed h-px" />

        <div className="h-[38%] p-6 flex flex-col justify-between">
          <div>
            <span className="text-[10px] tracking-[0.28em] uppercase text-white/50">
              Invite Code
            </span>
            <div className="font-mono-mp text-white text-2xl tracking-[0.35em] mt-2">{code}</div>
          </div>

          <div className="flex items-end gap-[3px] h-10">
            {[3, 7, 4, 9, 5, 8, 4, 10, 6, 5, 9, 4, 7, 5, 8, 6, 9, 4, 7, 5, 8, 4, 10].map((h, i) => (
              <div
                key={i}
                className="bg-white/70"
                style={{ width: 2, height: `${String(h * 4)}px` }}
              />
            ))}
          </div>
        </div>

        <div
          className="absolute inset-y-0 -left-1/3 w-1/3 pointer-events-none opacity-30"
          style={{
            background: "linear-gradient(90deg, transparent, rgba(255,255,255,0.15), transparent)",
            transform: "skewX(-20deg)",
          }}
        />
      </div>
    </motion.div>
  );
}

export default MovieTicket;

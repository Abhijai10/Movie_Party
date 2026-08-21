/* eslint-disable react/no-unknown-property */
import { Canvas, useFrame } from "@react-three/fiber";
import { CatmullRomCurve3, DoubleSide, Vector3 } from "three";
import { useState, useRef } from "react";
import type { Group, Mesh } from "three";

type FilmReelSceneProps = {
  reducedMotion?: boolean;
};

function ReelAssembly({ reducedMotion }: FilmReelSceneProps) {
  const assembly = useRef<Group>(null);
  const reel = useRef<Group>(null);
  const strip = useRef<Mesh>(null);

  useFrame((state, delta) => {
    if (!assembly.current || !reel.current || !strip.current) {
      return;
    }

    const pointerX = state.pointer.x * 0.12;
    const pointerY = state.pointer.y * 0.08;
    assembly.current.rotation.y += (pointerX - assembly.current.rotation.y) * 0.025;
    assembly.current.rotation.x += (-0.16 + pointerY - assembly.current.rotation.x) * 0.025;
    if (!reducedMotion) {
      reel.current.rotation.z += delta * 0.16;
      strip.current.rotation.y += delta * 0.035;
    }
  });

  const stripPath = new CatmullRomCurve3([
    new Vector3(-3.7, -0.25, -1.05),
    new Vector3(-2.6, 1.5, -0.55),
    new Vector3(-0.7, 1.05, 0.35),
    new Vector3(1.2, 0.15, 0.42),
    new Vector3(3.55, 1.18, -0.18),
  ]);

  return (
    <group ref={assembly} position={[0.5, -0.25, 0]} rotation={[-0.16, -0.24, 0.08]}>
      <mesh rotation={[-Math.PI / 2, 0, 0]} position={[0.5, -2.16, 0]} receiveShadow>
        <circleGeometry args={[4.8, 64]} />
        <shadowMaterial opacity={0.38} />
      </mesh>
      <group ref={reel} position={[0.35, -0.1, 0]} rotation={[Math.PI / 2, 0, 0]}>
        <mesh castShadow receiveShadow>
          <cylinderGeometry args={[2.14, 2.14, 0.28, 72]} />
          <meshStandardMaterial color="#161b20" metalness={0.92} roughness={0.26} />
        </mesh>
        <mesh position={[0, 0.17, 0]} castShadow>
          <cylinderGeometry args={[1.92, 1.92, 0.1, 72]} />
          <meshStandardMaterial color="#27313a" metalness={0.8} roughness={0.32} />
        </mesh>
        {[0, 1, 2, 3, 4, 5].map((index) => {
          const angle = (index / 6) * Math.PI * 2;
          return (
            <mesh
              key={index}
              position={[Math.cos(angle) * 1.14, 0.25, Math.sin(angle) * 1.14]}
            >
              <cylinderGeometry args={[0.46, 0.46, 0.42, 32]} />
              <meshStandardMaterial color="#07090b" metalness={0.35} roughness={0.48} />
            </mesh>
          );
        })}
        <mesh position={[0, 0.29, 0]} castShadow>
          <cylinderGeometry args={[0.44, 0.44, 0.34, 42]} />
          <meshStandardMaterial color="#4e6871" metalness={0.88} roughness={0.22} />
        </mesh>
      </group>
      <mesh ref={strip} position={[0.1, 0.12, -0.45]} castShadow>
        <tubeGeometry args={[stripPath, 96, 0.19, 8, false]} />
        <meshStandardMaterial color="#1e252c" metalness={0.62} roughness={0.36} side={DoubleSide} />
      </mesh>
      {Array.from({ length: 18 }, (_, index) => {
        const point = stripPath.getPoint(index / 17);
        return (
          <mesh key={index} position={[point.x, point.y + 0.08, point.z - 0.05]} rotation={[0.42, 0.2, -0.22]}>
            <boxGeometry args={[0.22, 0.11, 0.07]} />
            <meshStandardMaterial color="#090b0d" roughness={0.6} />
          </mesh>
        );
      })}
    </group>
  );
}

export function FilmReelScene({ reducedMotion = false }: FilmReelSceneProps) {
  const [webglReady, setWebglReady] = useState(false);

  return (
    <section className="film-reel-scene" aria-label="A rotating film reel and strip">
      <div className={webglReady ? "film-reel-fallback is-covered" : "film-reel-fallback"} aria-hidden="true">
        <span className="film-reel-fallback-disc" />
        <span className="film-reel-fallback-strip" />
      </div>
      <Canvas
        className="film-reel-canvas"
        dpr={[1, 1.5]}
        camera={{ position: [0, 0.15, 8.4], fov: 35 }}
        shadows
        gl={{ antialias: true, alpha: true, powerPreference: "high-performance" }}
        onCreated={() => setWebglReady(true)}
      >
        <ambientLight intensity={0.58} />
        <directionalLight position={[3.6, 4.8, 4.2]} intensity={2.6} color="#d7f6ff" castShadow />
        <pointLight position={[-3.8, 1.5, 1.4]} intensity={18} distance={8} color="#5B21FF" />
        <pointLight position={[3.2, -1.4, 2.8]} intensity={11} distance={7} color="#b9704e" />
        <ReelAssembly reducedMotion={reducedMotion} />
      </Canvas>
      <div className="film-scene-caption" aria-hidden="true">
        <span>Private cinema, perfectly in sync</span>
      </div>
    </section>
  );
}

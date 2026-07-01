// evo_sim - Copyright (c) 2026 Lens and Mix, LLC
// Licensed under the Apache License, Version 2.0. See LICENSE and NOTICE.
// More information: https://rkeithelliott.com
/**
 * worker.js — Simulation Web Worker
 *
 * Runs WASM simulation on a dedicated thread.
 * Sends data snapshots to main thread via structured clone (copy, not transfer).
 * Simpler and more reliable than transferable buffers — copy overhead is
 * negligible compared to the simulation and render work.
 */

import init, { SimWorld } from './pkg/evo_sim.js';

let sim      = null;
let SimWorld_cls = null;
let paused   = true;
let simSpeed = 1;
let wasmMem  = null;
let traceLive = false;   // when true, stream the trace buffer to the live panel each frame

// Frame rate throttle
const FRAME_MS = 1000 / 60;
let lastSendTime = 0;

async function start() {
    try {
        wasmMem      = await init();
        SimWorld_cls = SimWorld;
        sim          = new SimWorld();

        postMessage({
            type:        'ready',
            worldW:      sim.world_w(),
            worldH:      sim.world_h(),
            terrainCols: sim.terrain_cols(),
            terrainRows: sim.terrain_rows(),
            config:      sim.config_snapshot(),
        });

        loop();
    } catch(e) {
        postMessage({ type: 'error', message: e.toString() });
    }
}

function loop() {
    if (!paused) {
        for (let i = 0; i < simSpeed; i++) sim.update();
    }

    const now = Date.now();
    if (now - lastSendTime >= FRAME_MS) {
        lastSendTime = now;
        sendFrame();
    }

    setTimeout(loop, 0);
}

function sendFrame() {
    const mem = wasmMem.memory.buffer;

    // Copy buffer data into new typed arrays (structured clone)
    const tPtr = sim.terrain_buffer_ptr();
    const tLen = sim.terrain_buffer_len();
    const pPtr = sim.plant_buffer_ptr();
    const pLen = sim.plant_buffer_len();
    const hPtr = sim.herb_buffer_ptr();
    const hLen = sim.herb_buffer_len();
    const cPtr = sim.carn_buffer_ptr();
    const cLen = sim.carn_buffer_len();
    const hIdPtr = sim.herb_id_buffer_ptr();
    const hIdLen = sim.herb_id_buffer_len();
    const cIdPtr = sim.carn_id_buffer_ptr();
    const cIdLen = sim.carn_id_buffer_len();

    const terrainData = new Float32Array(mem, tPtr, tLen).slice();
    const plantData   = new Float32Array(mem, pPtr, pLen).slice();
    const herbData    = new Float32Array(mem, hPtr, hLen).slice();
    const carnData    = new Float32Array(mem, cPtr, cLen).slice();
    const herbIdData  = new Float32Array(mem, hIdPtr, hIdLen).slice();
    const carnIdData  = new Float32Array(mem, cIdPtr, cIdLen).slice();

    postMessage({
        type: 'frame',
        stats: {
            frame:           Number(sim.frame()),
            generation:      Number(sim.generation()),
            nHerbs:          sim.n_herbs_total(),
            nHerbsActive:    sim.n_herbs_active(),
            nPlantsMature:   sim.n_plants_mature(),
            nPlantsSeeding:  sim.n_plants_seedlings(),
            nPlantsSeeds:    sim.n_plants_seeds(),
            nPlantsActive:   sim.n_plants_active(),
            nCarns:          sim.n_carns_total(),
            nCarnsActive:    sim.n_carns_active(),
            diagEats:        sim.diag_eats(),
            diagStarvations: sim.diag_starvations(),
            diagAvgAge:      sim.diag_avg_age_at_death(),
            diagLocked:      sim.diag_plants_locked(),
            diagEnergy:      sim.diag_avg_energy(),
            totalEnergy:     sim.total_energy(),
            worldW:          sim.world_w(),
            worldH:          sim.world_h(),
            terrainCols:     sim.terrain_cols(),
            terrainRows:     sim.terrain_rows(),
            paused,
        },
        terrainData,
        plantData,
        herbData,
        carnData,
        herbIdData,
        carnIdData,
        tLen,
        pLen,
        hLen,
        cLen,
        traceText: traceLive ? sim.export_trace() : null,
    });
}

self.onmessage = function(e) {
    const msg = e.data;
    switch (msg.type) {
        case 'pause':         paused = true;  break;
        case 'resume':        paused = false; break;

        case 'step':
            // Single-tick advance. Always exactly one tick regardless of
            // simSpeed. Only meaningful while paused — the UI gates the Step
            // control, but we guard here too so a stray 'step' can't inject a
            // tick during normal playback. sendFrame() posts immediately,
            // bypassing the loop's frame-rate throttle (intended: we want the
            // stepped frame on screen right away).
            if (paused) {
                sim.update();
                sendFrame();
            }
            break;

        case 'set_sim_speed': simSpeed = msg.value; break;

        case 'reset':
            sim.regenerate(msg.seed ?? BigInt(Date.now()));
            postMessage({ type: 'reset_done',
                worldW: sim.world_w(), worldH: sim.world_h(),
                terrainCols: sim.terrain_cols(), terrainRows: sim.terrain_rows(),
                config: sim.config_snapshot() });
            break;

        case 'dev_world':
            sim = SimWorld_cls.dev_world();
            postMessage({ type: 'reset_done',
                worldW: sim.world_w(), worldH: sim.world_h(),
                terrainCols: sim.terrain_cols(), terrainRows: sim.terrain_rows(),
                config: sim.config_snapshot() });
            break;

        case 'plant_lab':
            sim = SimWorld_cls.plant_lab();
            postMessage({ type: 'reset_done',
                worldW: sim.world_w(), worldH: sim.world_h(),
                terrainCols: sim.terrain_cols(), terrainRows: sim.terrain_rows(),
                config: sim.config_snapshot() });
            break;

        case 'add_plant':
            sim.add_plant(msg.x, msg.y, msg.mature || false);
            sendFrame();
            break;

        case 'clear_plants':
            sim.clear_plants();
            sendFrame();
            break;

        case 'energy_audit':
            sim.set_energy_audit(msg.on);
            break;

        case 'energy_report':
            postMessage({ type: 'energy_report', data: sim.energy_report() });
            break;

        case 'main_world':
            // Full default world — mirror of dev_world. Fresh SimWorld() uses
            // Config::default (4000x2000, seed 12345, herbs/carns on). The client
            // re-pushes its herb/carn toggles after reset_done (intent wins).
            sim = new SimWorld_cls();
            postMessage({ type: 'reset_done',
                worldW: sim.world_w(), worldH: sim.world_h(),
                terrainCols: sim.terrain_cols(), terrainRows: sim.terrain_rows(),
                config: sim.config_snapshot() });
            break;

        case 'set_water_level':
            sim.set_water_level(msg.value);
            postMessage({ type: 'terrain_changed' });
            break;
        case 'set_hillshade':
            sim.set_hillshade_strength(msg.value);
            postMessage({ type: 'terrain_changed' });
            break;
        case 'set_nutrient_flow':  sim.set_nutrient_flow_rate(msg.value);  break;
        case 'set_seed_chance':    sim.set_plant_seed_chance(msg.value);   break;
        case 'set_plant_mutation': sim.set_plant_mutation_rate(msg.value); break;
        case 'set_plants_evolve':  sim.set_plants_evolve(msg.value);       break;
        case 'set_herb_mutation':  sim.set_herb_mutation_rate(msg.value);  break;
        case 'set_trace':          sim.set_trace_enabled(msg.value); traceLive = msg.value; break;
        case 'set_trace_focus':    sim.set_trace_focus(BigInt(msg.value || 0)); break;
        case 'set_tag':            sim.set_tag(BigInt(msg.value || 0), msg.kind || 0); break;
        case 'set_herbs_enabled':  sim.set_herbs_enabled(msg.value);       break;
        case 'set_carns_enabled':  sim.set_carns_enabled(msg.value);       break;

        case 'export_trace':
            postMessage({ type: 'trace_data',
                data:  sim.export_trace(),
                frame: Number(sim.frame()),
                gen:   Number(sim.generation()) });
            break;

        case 'export_csv':
            postMessage({ type: 'csv_data',
                data: sim.export_diag_csv(),
                gen:  Number(sim.generation()) });
            break;

        case 'export_agent_csv':
            postMessage({ type: 'agent_csv_data',
                data:  sim.export_agent_csv(),
                frame: Number(sim.frame()),
                gen:   Number(sim.generation()) });
            break;

        case 'inspect':
            postMessage({ type: 'inspect_data',
                kind: msg.kind, id: msg.id,
                data: sim.inspect(msg.kind, BigInt(msg.id)) });
            break;

        case 'get_pop_history':
            postMessage({ type: 'pop_history',
                data: sim.pop_history_data(),
                len:  sim.pop_history_len(),
                gen:  Number(sim.generation()) });
            break;
    }
};

start();

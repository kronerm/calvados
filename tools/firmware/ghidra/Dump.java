// Args: "dec" addr... | "str" regex | "xref" addr... | "callers" addr...
// Output goes to stdout lines prefixed with "@@".
import ghidra.app.script.GhidraScript;
import ghidra.app.decompiler.*;
import ghidra.program.model.address.*;
import ghidra.program.model.listing.*;
import ghidra.program.model.symbol.*;
import ghidra.program.model.data.*;
import java.util.*;
import java.util.regex.*;

public class Dump extends GhidraScript {
    DecompInterface dec;

    void out(String s) {
        for (String l : s.split("\n")) println("@@" + l);
    }

    Function fnAt(Address a) {
        Function f = getFunctionContaining(a);
        if (f == null) {
            disassemble(a);
            createFunction(a, null);
            f = getFunctionContaining(a);
        }
        return f;
    }

    @Override
    public void run() throws Exception {
        String[] args = getScriptArgs();
        String mode = args[0];
        dec = new DecompInterface();
        dec.openProgram(currentProgram);
        if (mode.equals("dec")) {
            for (int i = 1; i < args.length; i++) {
                Address a = toAddr(args[i]);
                Function f = fnAt(a);
                if (f == null) { out("no function at " + a); continue; }
                DecompileResults r = dec.decompileFunction(f, 120, monitor);
                out("==== " + f.getName() + " @ " + f.getEntryPoint());
                out(r.decompileCompleted() ? r.getDecompiledFunction().getC() : "decompile failed: " + r.getErrorMessage());
            }
        } else if (mode.equals("str")) {
            Pattern p = Pattern.compile(args[1]);
            for (Data d : currentProgram.getListing().getDefinedData(true)) {
                if (!(d.getValue() instanceof String)) continue;
                String s = (String) d.getValue();
                if (!p.matcher(s).find()) continue;
                StringBuilder sb = new StringBuilder();
                for (Reference r : getReferencesTo(d.getAddress())) {
                    Function f = getFunctionContaining(r.getFromAddress());
                    sb.append(" ").append(r.getFromAddress()).append(f == null ? "" : "(" + f.getEntryPoint() + ")");
                }
                out(d.getAddress() + " \"" + s.replace("\n", "\\n") + "\" <-" + sb);
            }
        } else if (mode.equals("dis")) {
            Address a = toAddr(args[1]), end = toAddr(args[2]);
            InstructionIterator it = currentProgram.getListing().getInstructions(a, true);
            while (it.hasNext()) {
                Instruction ins = it.next();
                if (ins.getAddress().compareTo(end) > 0) break;
                Function f = getFunctionContaining(ins.getAddress());
                String lbl = (f != null && f.getEntryPoint().equals(ins.getAddress())) ? "  <" + f.getName() + ">" : "";
                out(ins.getAddress() + ": " + ins + lbl);
            }
        } else if (mode.equals("lin")) {
          for (int pi = 1; pi + 1 < args.length; pi += 2) {
            Address a = toAddr(args[pi]), end = toAddr(args[pi+1]);
            while (a.compareTo(end) < 0) {
                Instruction ins = getInstructionAt(a);
                if (ins == null) { disassemble(a); ins = getInstructionAt(a); }
                if (ins == null) { a = a.add(2); continue; }
                Function f = getFunctionContaining(ins.getAddress());
                String lbl = (f != null && f.getEntryPoint().equals(ins.getAddress())) ? "  <" + f.getName() + ">" : "";
                out(ins.getAddress() + ": " + ins + lbl);
                a = a.add(ins.getLength());
            }
          }
        } else if (mode.equals("before")) {
            int n = Integer.parseInt(args[1]);
            for (int i = 2; i < args.length; i++) {
                Address a = toAddr(args[i]);
                Instruction ins = getInstructionAt(a);
                java.util.LinkedList<Instruction> l = new java.util.LinkedList<>();
                Instruction p = ins;
                for (int k = 0; k < n && p != null; k++) { l.addFirst(p); p = p.getPrevious(); }
                Function f = getFunctionContaining(a);
                out("---- " + a + (f == null ? "" : " in " + f.getName()));
                for (Instruction x : l) out("  " + x.getAddress() + ": " + x);
            }
        } else if (mode.equals("ptr")) {
            // find 32-bit LE words equal to any of the given values (Thumb: value|1 too)
            byte[] all = new byte[(int) currentProgram.getMemory().getSize()];
            currentProgram.getMemory().getBytes(currentProgram.getMinAddress(), all);
            for (int i = 1; i < args.length; i++) {
                long v = Long.decode(args[i]);
                StringBuilder sb = new StringBuilder();
                for (int o = 0; o + 4 <= all.length; o += 2) {
                    long w = (all[o] & 0xffL) | (all[o+1] & 0xffL) << 8 | (all[o+2] & 0xffL) << 16 | (all[o+3] & 0xffL) << 24;
                    if (w == v || w == (v | 1)) sb.append(" ").append(Long.toHexString(o));
                }
                out(Long.toHexString(v) + " at" + sb);
            }
        } else if (mode.equals("into")) {
            long lo = Long.decode(args[1]), hi = Long.decode(args[2]);
            for (Function f : currentProgram.getFunctionManager().getFunctions(toAddr(lo), true)) {
                if (f.getEntryPoint().getOffset() >= hi) break;
                StringBuilder sb = new StringBuilder();
                for (Reference r : getReferencesTo(f.getEntryPoint())) {
                    long from = r.getFromAddress().getOffset();
                    if (from >= lo && from < hi) continue;
                    Function g = getFunctionContaining(r.getFromAddress());
                    sb.append(" ").append(r.getFromAddress()).append(g == null ? "" : "(" + g.getName() + ")");
                }
                if (sb.length() > 0) out(f.getName() + " @ " + f.getEntryPoint() + " <-" + sb);
            }
        } else if (mode.equals("frag")) {
            int n = Integer.parseInt(args[1]);
            for (int i = 2; i < args.length; i++) {
                Address a = toAddr(args[i]);
                out("---- " + a);
                Instruction ins = getInstructionAt(a);
                if (ins == null) { disassemble(a); ins = getInstructionAt(a); }
                for (int k = 0; k < n && ins != null; k++) {
                    out("  " + ins.getAddress() + ": " + ins);
                    String m = ins.getMnemonicString();
                    if (n < 100 && (m.equals("bx") || m.startsWith("pop") || m.equals("b.w") || m.equals("b"))) break;
                    ins = ins.getNext();
                }
            }
        } else if (mode.equals("xref") || mode.equals("callers")) {
            for (int i = 1; i < args.length; i++) {
                Address a = toAddr(args[i]);
                StringBuilder sb = new StringBuilder();
                for (Reference r : getReferencesTo(a)) {
                    Function f = getFunctionContaining(r.getFromAddress());
                    sb.append(" ").append(r.getFromAddress()).append(f == null ? "" : "(" + f.getEntryPoint() + ")");
                }
                out(a + " <-" + sb);
            }
        }
    }
}

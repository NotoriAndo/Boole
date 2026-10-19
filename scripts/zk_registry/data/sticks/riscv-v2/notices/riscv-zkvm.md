# riscv-zkvm

Upstream: https://github.com/Verified-zkEVM/riscv-zkvm at 93ab6ef3a13b4c733c057e441eef2caca74bfb8e.
Licence: BSD-2-Clause.

## Upstream LICENSE

RISCV Sail Model

This Sail RISC-V architecture model, comprising all files and
directories except for the third party dependencies in the
'dependencies' directory (which include copies of their licences) and
except for the separately-licensed material listed under "Other
licensed material" below, is subject to the BSD two-clause licence
below.


Other licensed material
=======================

`RiscvZkvm/Rv64/` -- the RISC-V program-logic layer (`Word`, `Basic`,
`Instructions`, `Execution`, `Program`, `ZiskAccel`, and the `Logic/`
subtree) -- originated in the evm-asm project
(https://github.com/Verified-zkEVM/evm-asm) and was relocated here in
releases v0.2.0 and v0.3.0.  That material is

    Copyright (c) 2026 ZkSecurity

and is distributed under the MIT licence, a copy of which is in
RiscvZkvm/Rv64/LICENSE-MIT.  It is NOT covered by the BSD two-clause
licence below.


Copyright (c) 2017-2025
   ahadali5000
   Alasdair Armstrong
   Alexander Richardson
   Aril Computer Corp., for contributions by Scott Johnson
   Ben Marshall
   Bicheng Yang
   Bilal Sakhawat
   Brian Campbell
   Chris Casinghino
   Christopher Pulte
   Codasip, for contributions by Tim Hutt, Martin Berger and Ben Fletcher
   dylux
   eroom1966
   Google LLC, for contributions by its employees
   Hesham Almatary
   Jan Henrik Weinstock
   Jessica Clarke
   Jon French
   Jordan Carlin
   Martin Berger
   Michael Sammler
   Microsoft, for contributions by Robert Norton-Wright and Nathaniel Wesley Filardo
   Muhammad Bilal Sakhawat
   Nathaniel Wesley Filardo
   Paul A. Clarke
   Peter Rugg
   Peter Sewell
   Philipp Tomsich
   Prashanth Mundkur
   Rafael Sene
   Rishiyur S. Nikhil (Bluespec, Inc.)
   Robert Norton-Wright
   Scott Johnson (Aril Inc.)
   Shaked Flur
   Thibaut Pérami
   Thomas Bauereiss
   VRULL GmbH, for contributions by its employees
   William McSpaddden
   Xinlai Wan

For a complete list of authors run this command:

    git shortlog --summary --numbered --email

All rights reserved.

This software was developed by the above within the Rigorous
Engineering of Mainstream Systems (REMS) project, partly funded by
EPSRC grant EP/K008528/1, at the Universities of Cambridge and
Edinburgh.

This software was developed by SRI International and the University of
Cambridge Computer Laboratory (Department of Computer Science and
Technology) under DARPA/AFRL contract FA8650-18-C-7809 ("CIFV"), and
under DARPA contract HR0011-18-C-0016 ("ECATS") as part of the DARPA
SSITH research programme.

This project has received funding from the European Research Council
(ERC) under the European Union’s Horizon 2020 research and innovation
programme (grant agreement 789108, ELVER).

------------------------------------------------------------------

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions
are met:
1. Redistributions of source code must retain the above copyright
   notice, this list of conditions and the following disclaimer.
2. Redistributions in binary form must reproduce the above copyright
   notice, this list of conditions and the following disclaimer in
   the documentation and/or other materials provided with the
   distribution.

THIS SOFTWARE IS PROVIDED BY THE AUTHOR AND CONTRIBUTORS ``AS IS''
AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED
TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A
PARTICULAR PURPOSE ARE DISCLAIMED.  IN NO EVENT SHALL THE AUTHOR OR
CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT
LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF
USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND
ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY,
OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT
OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
SUCH DAMAGE.

Sail extraction provenance: riscv/sail-riscv 27224ccb2290f022e46213c05b3e72e8a9ea635e, BSD-2-Clause; extraction bytes are unchanged except B2's mechanical `sailAssert` rename and the fixed-platform definitions. The full Rv64 MIT notice is reproduced below.


## Rv64 upstream LICENSE-MIT

MIT License

Copyright (c) 2026 ZkSecurity

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.

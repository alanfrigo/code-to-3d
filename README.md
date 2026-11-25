# code-to-3d

Harness local de produção de assets. O [plano](PLANO-HARNESS.md) separa orquestração e contrato em Rust, operações 3D em Blender/Python e verificação na engine. Neste PR, o escopo é apenas a bancada **M0**; Godot é o alvo de verificação, não uma decisão sobre a engine do jogo.

## Executar o round-trip M0

Linux x86-64, Rustup e ferramentas padrão (`curl`, `jq`, `sha256sum`, `tar`, `unzip`). O Rust está fixado em `rust-toolchain.toml`. O bootstrap baixa e verifica os arquivos oficiais de Blender e Godot de `toolchain.lock.json`: use-o uma vez por instalação. O comando Rust verifica versão e SHA-256 dos executáveis antes de processar a fixture. Binários de distro ou de outra patch não são substitutos para este perfil.

```bash
mkdir -p out
bash scripts/fetch-toolchain.sh out/toolchain
cargo run --locked -- roundtrip \
  --blender out/toolchain/blender-5.2.5-linux-x64/blender \
  --godot out/toolchain/Godot_v4.7.2-stable_linux.x86_64 \
  --out out/m0
```

`--out` precisa ser um diretório novo, com o pai já existente. Para repetir, mude o nome. Não há GPU de ML, modelo, asset externo ou sessão Blender/Godot interativa no caminho. O comando executa apenas a fixture do repositório; não é um sandbox para arquivos ou scripts externos.

## O que a prova mede

1. Blender gera uma malha assimétrica com skin, dois ossos, um socket e um clipe de 1 s, e exporta `probe/fixture.glb`.
2. Godot importa o GLB com o importador real de editor. Uma execução separada instancia a cena importada, amostra o clipe e escreve `observed.json`.
3. Rust compara as observações ao [perfil](profiles/godot-glb.json): bounds em metros após a conversão de eixos, presença de skin, hierarquia e socket, duração e poses em 0, 0,5 e 1 s. Tolerâncias: 1 mm em posição, 0,1 ms em tempo.

Um `PASS` produz `report.json` com observações, toolchain e SHA-256 do GLB. Exit `3` indica divergências com relatório; `1`, falha de execução ou evidência ausente/inválida (consulte os logs das etapas); `2`, erro de uso da CLI. O CI executa o mesmo comando e `cargo test --locked`. Este smoke é headless: não valida aparência, deformação artística ou transições de gameplay.

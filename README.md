# Fila de Impressão

Janelinha para ver e limpar a fila de impressão do Windows. Rust + egui, um exe só.

- Lista os jobs da impressora escolhida, atualiza a cada 2 s.
- **Cancelar** um job ou **Limpar tudo**: usa a API do spooler, sem admin.
- **Forçar**: para job que não sai. Pede admin, para o spooler, apaga os arquivos em `System32\spool\PRINTERS` e liga o spooler de novo. Apaga a fila de **todas** as impressoras.

## Rodar

```bash
cargo run --release
```

## Testar

```bash
cargo test
```

O teste que mexe em fila de verdade fica desligado por padrão. Ele cria jobs pausados na `Microsoft Print to PDF` e limpa; nada é impresso.

```bash
cargo test -- --ignored
```

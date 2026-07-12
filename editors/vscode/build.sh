#!/bin/bash
set -e

# Navega para o diretório da extensão
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$DIR"

echo "=== Instalando dependências npm ==="
npm install

echo "=== Compilando extensão TypeScript ==="
npm run compile

echo "=== Concluído! ==="

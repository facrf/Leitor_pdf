# HTTPS com proxy reverso

HTTP Basic protege o acesso lógico, mas não cifra usuário, senha ou livros em trânsito. Para acessar a Estante Livre fora da própria máquina, mantenha a porta `20000` fora da internet e termine TLS em uma VPN ou proxy reverso.

## Caddy

Coloque Caddy e `estante-livre` na mesma rede Docker. Um `Caddyfile` mínimo é:

```caddyfile
biblioteca.example.com {
    encode zstd gzip
    reverse_proxy estante-livre:20000

    header {
        Strict-Transport-Security "max-age=31536000; includeSubDomains"
        X-Content-Type-Options "nosniff"
        Referrer-Policy "no-referrer"
    }
}
```

O DNS de `biblioteca.example.com` precisa apontar para o proxy, e as portas `80` e `443` precisam chegar ao Caddy para emissão automática do certificado. Não publique `20000:20000` nesse cenário; use apenas `expose: ["20000"]` na rede interna. Para manter acesso de emergência somente no host, publique `127.0.0.1:20000:20000`.

## Traefik

Com o Traefik já configurado com o resolvedor `letsencrypt` e a rede externa `proxy`, acrescente ao serviço:

```yaml
services:
  estante-livre:
    networks:
      - proxy
    expose:
      - "20000"
    labels:
      - traefik.enable=true
      - traefik.http.routers.estante.rule=Host(`biblioteca.example.com`)
      - traefik.http.routers.estante.entrypoints=websecure
      - traefik.http.routers.estante.tls=true
      - traefik.http.routers.estante.tls.certresolver=letsencrypt
      - traefik.http.services.estante.loadbalancer.server.port=20000

networks:
  proxy:
    external: true
```

No Portainer, as labels entram no editor do Stack. Confirme que a rede `proxy` já existe e que o nome do resolvedor corresponde ao Traefik instalado.

## Verificação

1. Ative `AUTH_USERNAME` e uma senha longa em `AUTH_PASSWORD`.
2. Abra somente `https://biblioteca.example.com` e confirme o diálogo de autenticação.
3. Verifique que `http://` redireciona para HTTPS.
4. Confirme que a porta `20000` não responde pelo endereço público.
5. Baixe um livro e um backup para testar arquivos grandes e cabeçalhos `Range` através do proxy.
6. Consulte `https://biblioteca.example.com/api/health`; a resposta esperada é `{"status":"ok","offline_capable":true}`.

As rotas públicas por token continuam dispensando Basic por projeto, mas passam pelo TLS do proxy. Revogue links que não forem mais necessários.

# TURN Server Setup Guide

TURN (Traversal Using Relays around NAT) servers are required when peers cannot establish direct WebRTC connections due to symmetric NATs or restrictive firewalls.

## When You Need TURN

- **Symmetric NAT**: Common in corporate networks and mobile carriers
- **Strict Firewalls**: Blocking UDP traffic
- **VPN connections**: Often use symmetric NAT
- **~10-15% of connections** typically require TURN relay

## Options

### Option 1: Public TURN Services

#### Twilio Network Traversal Service
```toml
# gateway/config.toml
[webrtc.turn]
urls = ["turn:global.turn.twilio.com:3478?transport=udp"]
username = "${TWILIO_ACCOUNT_SID}"
credential = "${TWILIO_AUTH_TOKEN}"
```

#### Metered.ca
```toml
[webrtc.turn]
urls = [
    "turn:a.relay.metered.ca:80",
    "turn:a.relay.metered.ca:443",
    "turns:a.relay.metered.ca:443"
]
username = "${METERED_USERNAME}"
credential = "${METERED_CREDENTIAL}"
```

### Option 2: Self-Hosted coturn

#### Installation

```bash
# Ubuntu/Debian
sudo apt-get install coturn

# macOS
brew install coturn

# Docker
docker run -d --network=host coturn/coturn
```

#### Configuration

Create `/etc/turnserver.conf`:

```ini
# Network settings
listening-port=3478
tls-listening-port=5349
listening-ip=0.0.0.0
external-ip=YOUR_PUBLIC_IP

# Authentication
lt-cred-mech
user=shadowmesh:your-secure-password
realm=shadowmesh.network

# Security
fingerprint
no-multicast-peers
no-cli

# Logging
log-file=/var/log/turnserver.log
verbose

# TLS (recommended for production)
cert=/etc/letsencrypt/live/turn.example.com/fullchain.pem
pkey=/etc/letsencrypt/live/turn.example.com/privkey.pem

# Performance
total-quota=100
bps-capacity=0
stale-nonce=600

# Allowed peer IPs (restrict to your network if needed)
# denied-peer-ip=10.0.0.0-10.255.255.255
# denied-peer-ip=172.16.0.0-172.31.255.255
# denied-peer-ip=192.168.0.0-192.168.255.255
```

#### Start the Server

```bash
# Systemd
sudo systemctl enable coturn
sudo systemctl start coturn

# Manual
turnserver -c /etc/turnserver.conf
```

### Option 3: Kubernetes Deployment

```yaml
# k8s/turn-server.yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: coturn
  namespace: shadowmesh
spec:
  replicas: 2
  selector:
    matchLabels:
      app: coturn
  template:
    metadata:
      labels:
        app: coturn
    spec:
      containers:
        - name: coturn
          image: coturn/coturn:latest
          ports:
            - containerPort: 3478
              protocol: UDP
            - containerPort: 3478
              protocol: TCP
            - containerPort: 5349
              protocol: TCP
          args:
            - "-n"
            - "--log-file=stdout"
            - "--lt-cred-mech"
            - "--fingerprint"
            - "--no-multicast-peers"
            - "--realm=shadowmesh.network"
            - "--user=shadowmesh:$(TURN_PASSWORD)"
          env:
            - name: TURN_PASSWORD
              valueFrom:
                secretKeyRef:
                  name: turn-credentials
                  key: password
          resources:
            limits:
              cpu: "1"
              memory: 512Mi
            requests:
              cpu: 100m
              memory: 128Mi
---
apiVersion: v1
kind: Service
metadata:
  name: coturn
  namespace: shadowmesh
spec:
  type: LoadBalancer
  ports:
    - name: turn-udp
      port: 3478
      targetPort: 3478
      protocol: UDP
    - name: turn-tcp
      port: 3478
      targetPort: 3478
      protocol: TCP
    - name: turns
      port: 5349
      targetPort: 5349
      protocol: TCP
  selector:
    app: coturn
```

## Configuration in ShadowMesh

### Gateway Configuration

```toml
# gateway/config.toml
[signaling]
enabled = true

[webrtc]
enabled = true

[webrtc.stun]
servers = [
    "stun:stun.l.google.com:19302",
    "stun:stun1.l.google.com:19302"
]

[webrtc.turn]
enabled = true
urls = ["turn:turn.shadowmesh.network:3478"]
username = "${TURN_USERNAME}"
credential = "${TURN_CREDENTIAL}"
```

### Browser SDK Configuration

```javascript
const config = new ClientConfig('wss://gateway.example.com/ws/signaling');

// Add TURN servers
config.addIceServer({
    urls: ['turn:turn.example.com:3478'],
    username: 'shadowmesh',
    credential: 'your-credential'
});

// Add TURNS (TLS) for restrictive networks
config.addIceServer({
    urls: ['turns:turn.example.com:5349'],
    username: 'shadowmesh',
    credential: 'your-credential'
});
```

### Node Runner Configuration

```toml
# node-config.toml
[webrtc]
enabled = true
port = 4002

[webrtc.stun]
servers = ["stun:stun.l.google.com:19302"]

[webrtc.turn]
enabled = true
url = "turn:turn.shadowmesh.network:3478"
username = "${TURN_USERNAME}"
credential = "${TURN_CREDENTIAL}"
```

## Testing TURN Configuration

### Using Trickle ICE

1. Go to https://webrtc.github.io/samples/src/content/peerconnection/trickle-ice/
2. Add your TURN server credentials
3. Click "Gather candidates"
4. Verify you see `relay` candidates

### Command Line Testing

```bash
# Test STUN
turnutils_stunclient stun.l.google.com

# Test TURN
turnutils_uclient -t -u shadowmesh -w password turn.example.com
```

### Verify in Browser Console

```javascript
const pc = new RTCPeerConnection({
    iceServers: [{
        urls: 'turn:turn.example.com:3478',
        username: 'shadowmesh',
        credential: 'password'
    }]
});

pc.onicecandidate = (e) => {
    if (e.candidate) {
        console.log('Candidate:', e.candidate.candidate);
        if (e.candidate.candidate.includes('relay')) {
            console.log('✓ TURN is working!');
        }
    }
};

pc.createDataChannel('test');
pc.createOffer().then(o => pc.setLocalDescription(o));
```

## Security Considerations

1. **Use strong credentials**: Generate random passwords
2. **Enable TLS**: Use TURNS (port 5349) for encrypted relay
3. **Rate limiting**: Configure `total-quota` and `user-quota`
4. **IP restrictions**: Limit `denied-peer-ip` ranges
5. **Rotate credentials**: Update passwords periodically
6. **Monitor usage**: Check logs for abuse

## Bandwidth Considerations

TURN servers relay all media traffic, which can be expensive:

| Quality | Bandwidth per connection |
|---------|-------------------------|
| Audio only | ~50 Kbps |
| Data channel | ~100 Kbps |
| Video (SD) | ~1 Mbps |
| Video (HD) | ~3 Mbps |

For ShadowMesh (data-only), expect ~100 Kbps per relayed connection.

## Troubleshooting

### No relay candidates
- Check firewall allows UDP 3478
- Verify credentials are correct
- Ensure TURN server is reachable

### Connection timeout
- Increase ICE gathering timeout
- Add multiple TURN servers for redundancy
- Check server logs for errors

### Poor performance
- Use geographically close TURN servers
- Consider multiple regional deployments
- Monitor server CPU/bandwidth

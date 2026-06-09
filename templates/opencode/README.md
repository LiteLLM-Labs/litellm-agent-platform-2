# opencode behind the Anthropic Managed Agents API

Exposes [opencode](https://opencode.ai) through the Anthropic Managed Agents API spec. Point the LAP SDK at this server — change only `api_base`/`api_key`, no new integration code.

## Quickstart

### Docker

```bash
docker build -t opencode-anthropic-server .
docker run -p 8080:8080 \
  -e LITELLM_BASE_URL=https://your-gateway/v1 \
  -e LITELLM_API_KEY=sk-... \
  -e LITELLM_MODELS=claude-sonnet-4-6 \
  opencode-anthropic-server
```

### Local

```bash
npm install
ANTHROPIC_API_KEY=sk-ant-... npm start
```

Health check: `GET /health` → `{"ok":true,"opencode":true}`

## LAP SDK

```rust
let lap = Lap::new(LapConfig {
    anthropic_api_key: Some("any-key".into()),
    anthropic_base_url: "http://localhost:8080".into(),
    ..LapConfig::default()
});

let agent = lap.beta().agents().create(CreateAgentParams {
    name: "assistant".into(),
    model: AgentModel::from("claude-sonnet-4-6"),
    system: "You are helpful.".into(),
    ..Default::default()
}).await?;

let session = lap.beta().sessions().create(CreateSessionParams {
    agent: agent.id.clone(),
    ..Default::default()
}).await?;

lap.beta().sessions().events().send(&session.id, SendEventsParams {
    events: vec![json!({"type":"user.message","content":"Hello"})],
}).await?;

let mut stream = lap.beta().sessions().events().stream(&session.id).await?;
while let Some(Ok(ev)) = stream.next().await {
    if ev.event_type == "session.status_idle" { break; }
    if ev.event_type == "agent.message" { /* print text */ }
}
```

## Environment variables

| Var | Default | Purpose |
|-----|---------|---------|
| `PORT` | `8080` | listen port |
| `WORKDIR` | `/tmp/opencode-workspace` | per-agent config directory |
| `DB_PATH` | `/data/agents.db` | SQLite agent store |
| `ANTHROPIC_API_KEY` | — | native Anthropic key (alternative to LiteLLM) |
| `LITELLM_BASE_URL` | — | LiteLLM gateway base URL (include `/v1`) |
| `LITELLM_API_KEY` | — | LiteLLM gateway key |
| `LITELLM_MODELS` | `claude-sonnet-4-6` | comma-separated models to register |
| `OPENSANDBOX_API_URL` | — | OpenSandbox controller URL (enables sandboxed execution) |
| `OPENSANDBOX_API_KEY` | — | OpenSandbox API key |
| `OPENSANDBOX_IMAGE` | — | sandbox execd image |

## Deploy on EKS with OpenSandbox

Full production deployment: opencode-anthropic-server + [OpenSandbox](https://github.com/opensandbox-group/OpenSandbox) on EKS. Agent commands and file operations run in isolated sandbox containers instead of on the server host.

See [`docs/eks-deployment.md`](docs/eks-deployment.md) for the full guide. Summary:

### 1. EKS cluster

```yaml
# cluster.yaml
apiVersion: eksctl.io/v1alpha5
kind: ClusterConfig
metadata:
  name: opensandbox
  region: eu-west-1
  version: "1.32"
managedNodeGroups:
  - name: workers
    instanceType: t3.medium
    desiredCapacity: 2
    iam:
      attachPolicyARNs:
        - arn:aws:iam::aws:policy/AmazonEKSWorkerNodePolicy
        - arn:aws:iam::aws:policy/AmazonEKS_CNI_Policy
        - arn:aws:iam::aws:policy/AmazonEC2ContainerRegistryReadOnly
        - arn:aws:iam::aws:policy/service-role/AmazonEBSCSIDriverPolicy
```

```bash
eksctl create cluster -f cluster.yaml
```

### 2. OpenSandbox

```bash
git clone https://github.com/opensandbox-group/OpenSandbox.git
cd OpenSandbox/kubernetes
helm dependency build charts/opensandbox

# Generate an API key
OPENSANDBOX_API_KEY=$(openssl rand -hex 32)

helm install opensandbox charts/opensandbox \
  --namespace opensandbox-system --create-namespace \
  -f - <<EOF
opensandbox-controller:
  controller:
    snapshot:
      containerdSocketPath: ""   # required: v0.2.0 chart bug fix
opensandbox-server:
  server:
    replicaCount: 1
    resources:
      requests: { cpu: 500m, memory: 512Mi }
      limits:   { cpu: "2",  memory: 2Gi  }
  configToml: |
    [server]
    host = "0.0.0.0"
    port = 80
    api_key = "$OPENSANDBOX_API_KEY"
    [runtime]
    type = "kubernetes"
    execd_image = "sandbox-registry.cn-zhangjiakou.cr.aliyuncs.com/opensandbox/execd:v1.0.18"
    [kubernetes]
    namespace = "opensandbox"
    workload_provider = "batchsandbox"
    batchsandbox_template_file = "/etc/opensandbox/example.batchsandbox-template.yaml"
    [egress]
    image = "sandbox-registry.cn-zhangjiakou.cr.aliyuncs.com/opensandbox/egress:v1.0.12"
    mode = "dns+nft"
EOF
```

> **Chart bugs** (v0.2.0): controller crashes without `containerdSocketPath: ""`; default server requests 4Gi RAM which won't fit t3.medium — both fixed in values above.

### 3. EBS CSI driver (required for PVC)

```bash
eksctl utils associate-iam-oidc-provider --cluster opensandbox --region eu-west-1 --approve
eksctl create iamserviceaccount \
  --name ebs-csi-controller-sa --namespace kube-system \
  --cluster opensandbox --region eu-west-1 \
  --attach-policy-arn arn:aws:iam::aws:policy/service-role/AmazonEBSCSIDriverPolicy \
  --approve --role-only --role-name AmazonEKS_EBS_CSI_DriverRole

ROLE_ARN=$(aws iam get-role --role-name AmazonEKS_EBS_CSI_DriverRole --query 'Role.Arn' --output text)
aws eks create-addon --cluster-name opensandbox --addon-name aws-ebs-csi-driver \
  --service-account-role-arn "$ROLE_ARN" --region eu-west-1

kubectl apply -f - <<'EOF'
apiVersion: storage.k8s.io/v1
kind: StorageClass
metadata:
  name: gp3
  annotations:
    storageclass.kubernetes.io/is-default-class: "true"
provisioner: ebs.csi.aws.com
parameters:
  type: gp3
volumeBindingMode: WaitForFirstConsumer
EOF
```

### 4. opencode-anthropic-server

```bash
# Build and push to ECR
ECR=<account>.dkr.ecr.<region>.amazonaws.com/opencode-anthropic-server
aws ecr create-repository --repository-name opencode-anthropic-server --region eu-west-1
aws ecr get-login-password --region eu-west-1 | docker login --username AWS --password-stdin $ECR
docker build --platform linux/amd64 -t $ECR:latest .
docker push $ECR:latest

# Deploy
kubectl create secret generic opencode-server-secrets -n opensandbox-system \
  --from-literal=LITELLM_API_KEY=<key> \
  --from-literal=OPENSANDBOX_API_KEY=<key-from-step-2>

kubectl apply -f - <<EOF
apiVersion: v1
kind: PersistentVolumeClaim
metadata:
  name: opencode-data
  namespace: opensandbox-system
spec:
  accessModes: [ReadWriteOnce]
  storageClassName: gp3
  resources:
    requests:
      storage: 5Gi
---
apiVersion: apps/v1
kind: Deployment
metadata:
  name: opencode-anthropic-server
  namespace: opensandbox-system
spec:
  replicas: 1
  selector:
    matchLabels:
      app: opencode-anthropic-server
  template:
    metadata:
      labels:
        app: opencode-anthropic-server
    spec:
      initContainers:
        - name: db-cleanup
          image: busybox
          command: ["sh", "-c", "rm -f /data/agents.db-shm /data/agents.db-wal"]
          volumeMounts:
            - name: data
              mountPath: /data
      containers:
        - name: server
          image: $ECR:latest
          ports:
            - containerPort: 8080
          env:
            - name: WORKDIR
              value: /tmp/opencode-workspace
            - name: DB_PATH
              value: /data/agents.db
            - name: OPENSANDBOX_API_URL
              value: http://opensandbox-server.opensandbox-system.svc.cluster.local
            - name: OPENSANDBOX_IMAGE
              value: sandbox-registry.cn-zhangjiakou.cr.aliyuncs.com/opensandbox/execd:v1.0.18
            - name: LITELLM_BASE_URL
              value: https://your-gateway/v1
            - name: LITELLM_MODELS
              value: claude-sonnet-4-6
            - name: LITELLM_API_KEY
              valueFrom:
                secretKeyRef:
                  name: opencode-server-secrets
                  key: LITELLM_API_KEY
            - name: OPENSANDBOX_API_KEY
              valueFrom:
                secretKeyRef:
                  name: opencode-server-secrets
                  key: OPENSANDBOX_API_KEY
          volumeMounts:
            - name: data
              mountPath: /data
          resources:
            requests: { cpu: 250m, memory: 512Mi }
            limits:   { cpu: "1",  memory: 2Gi  }
      volumes:
        - name: data
          persistentVolumeClaim:
            claimName: opencode-data
---
apiVersion: v1
kind: Service
metadata:
  name: opencode-anthropic-server
  namespace: opensandbox-system
  annotations:
    service.beta.kubernetes.io/aws-load-balancer-type: nlb
spec:
  type: LoadBalancer
  selector:
    app: opencode-anthropic-server
  ports:
    - port: 80
      targetPort: 8080
EOF
```

### 5. Verify

```bash
LB=$(kubectl -n opensandbox-system get svc opencode-anthropic-server \
  -o jsonpath='{.status.loadBalancer.ingress[0].hostname}')

curl -s http://$LB/health   # {"ok":true,"opencode":true}
```

Then point the LAP SDK at `http://$LB` with model `claude-sonnet-4-6`.

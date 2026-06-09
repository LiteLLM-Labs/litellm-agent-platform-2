# Deployment Guide: opencode-anthropic-server + OpenSandbox on EKS

## 1. Overview

This stack runs [opencode](https://opencode.ai) as a sandboxed AI agent backend behind the **Anthropic Managed Agents API spec**. Any client that speaks that spec — including the LiteLLM Agent Platform (LAP) SDK's `claude_managed_agents` runtime — drives it with zero code changes.

**Components:**

- **opencode-anthropic-server** — Node.js Express app that translates the Anthropic Managed Agents API (`POST /v1/agents`, `POST /v1/sessions`, SSE stream, etc.) into opencode. It spawns `opencode serve` as a child process, persists agent/session state in SQLite, and rewrites opencode's SSE event stream into Anthropic event shapes. Deployed as a Kubernetes Deployment+Service+PVC in the `opensandbox-system` namespace.

- **OpenSandbox** — Kubernetes operator (Alibaba/CNCF Landscape) that manages sandbox lifecycle via BatchSandbox CRDs. The opencode-anthropic-server proxies the agent's command and file operations into sandbox containers via OpenSandbox's HTTP API, so code execution is isolated from the server host. Deployed via Helm in the same namespace.

- **LiteLLM gateway** (external, already running) — Routes model calls. The opencode-anthropic-server configures opencode's `litellm` provider to point at it; agents address models as `litellm/<model>`.

**Data flow:**

```
LAP SDK (claude_managed_agents)
  → opencode-anthropic-server  (Anthropic Managed Agents API, port 80)
      → opencode serve          (child process, port 4096)
          → OpenSandbox server  (sandbox exec/file ops, in-cluster HTTP)
              → BatchSandbox pods (actual isolated containers)
          → LiteLLM gateway     (model calls)
```

**Deployment target:** EKS cluster `opensandbox`, region `eu-west-1`, Kubernetes 1.32, managed node group `t3.medium`.

---

## 2. Prerequisites

### Tools

```bash
# AWS CLI v2
aws --version   # >= 2.x

# eksctl (EKS cluster management)
curl --silent --location "https://github.com/eksctl-io/eksctl/releases/latest/download/eksctl_$(uname -s)_amd64.tar.gz" | tar xz -C /tmp
sudo mv /tmp/eksctl /usr/local/bin

# kubectl
curl -LO "https://dl.k8s.io/release/$(curl -L -s https://dl.k8s.io/release/stable.txt)/bin/linux/amd64/kubectl"
sudo install -o root -g root -m 0755 kubectl /usr/local/bin/kubectl

# Helm 3
curl https://raw.githubusercontent.com/helm/helm/main/scripts/get-helm-3 | bash

# Docker (for building the server image)
docker --version   # >= 20.x
```

### AWS account requirements

- IAM user/role with permissions to create EKS clusters, EC2 instances, VPCs, IAM roles, and ECR repositories.
- Default VPC in `eu-west-1` is NOT used — eksctl creates a dedicated VPC. Verify you have not hit the default VPC limit (default is 5 VPCs per region; the cluster needs one). Check with:

  ```bash
  aws ec2 describe-vpcs --region eu-west-1 --query 'length(Vpcs)'
  ```

  If at the limit, delete an unused VPC or request a quota increase before proceeding.

- ECR access to push and pull images. The managed node group's IAM role needs `AmazonEC2ContainerRegistryReadOnly`.

---

## 3. EKS Cluster Setup

### 3.1 Create the cluster config

Save as `cluster.yaml`:

```yaml
apiVersion: eksctl.io/v1alpha5
kind: ClusterConfig
metadata:
  name: opensandbox
  region: eu-west-1
  version: "1.32"

managedNodeGroups:
  - name: workers
    instanceType: t3.medium
    minSize: 2
    maxSize: 4
    desiredCapacity: 3
    volumeSize: 50
    iam:
      attachPolicyARNs:
        - arn:aws:iam::aws:policy/AmazonEKSWorkerNodePolicy
        - arn:aws:iam::aws:policy/AmazonEKS_CNI_Policy
        - arn:aws:iam::aws:policy/AmazonEC2ContainerRegistryReadOnly
        - arn:aws:iam::aws:policy/service-role/AmazonEBSCSIDriverPolicy
```

The `AmazonEBSCSIDriverPolicy` is required for EBS volumes used by the opencode-anthropic-server PVC (see section 5.3).

### 3.2 Create the cluster

```bash
eksctl create cluster -f cluster.yaml
```

This takes 15–20 minutes. eksctl creates a dedicated VPC, subnets, and the managed node group. On completion it updates your kubeconfig automatically.

Verify:

```bash
kubectl get nodes
# NAME                                         STATUS   ROLES    AGE   VERSION
# ip-192-168-x-x.eu-west-1.compute.internal   Ready    <none>   2m    v1.32.x
```

### 3.3 Known gotchas

**VPC limit:** eksctl creates a new VPC for the cluster. If your account is at the 5-VPC limit in `eu-west-1`, the cluster creation fails with a VPC quota error. Delete an unused VPC or request an increase via the EC2 console under `Limits → VPCs per region`.

**Node capacity:** t3.medium has 2 vCPU / 4 GiB RAM. OpenSandbox server's default Helm values request 4 GiB RAM per replica — this fills a node entirely before the opencode server and sandbox pods can schedule. The resource patch in section 4.2 is required; without it pods remain in `Pending`.

---

## 4. OpenSandbox Deployment

### 4.1 Clone the repo and build dependencies

```bash
git clone https://github.com/opensandbox-group/OpenSandbox.git
cd OpenSandbox/kubernetes

# Build Helm chart dependencies (the top-level chart depends on sub-charts via file:// refs)
helm dependency build charts/opensandbox
```

### 4.2 Create the values override file

The default chart ships with two problems on t3.medium clusters (see below for explanation). Both are fixed via values override. Save as `opensandbox-values.yaml`:

```yaml
opensandbox-controller:
  controller:
    logLevel: info
    replicaCount: 1
    snapshot:
      # Blank out the containerd socket path to suppress the --containerd-socket-path flag.
      # The v0.2.0 chart passes this flag unconditionally when the value is non-empty,
      # but the controller binary in the deployed image no longer accepts it — the pod
      # crashes immediately with "unknown flag: --containerd-socket-path".
      containerdSocketPath: ""
      imageCommitterImage: sandbox-registry.cn-zhangjiakou.cr.aliyuncs.com/opensandbox/image-committer:v0.1.0

opensandbox-server:
  server:
    replicaCount: 1   # reduce for t3.medium; scale up when nodes are larger
    resources:
      requests:
        cpu: "500m"
        memory: "512Mi"
      limits:
        cpu: "2"
        memory: "2Gi"
  configToml: |
    [server]
    host = "0.0.0.0"
    port = 80
    api_key = "REPLACE_WITH_YOUR_API_KEY"

    [log]
    level = "INFO"

    [runtime]
    type = "kubernetes"
    execd_image = "sandbox-registry.cn-zhangjiakou.cr.aliyuncs.com/opensandbox/execd:v1.0.18"

    [kubernetes]
    kubeconfig_path = ""
    namespace = "opensandbox"
    informer_enabled = true
    informer_resync_seconds = 300
    informer_watch_timeout_seconds = 60
    snapshot_create_timeout_seconds = 900
    workload_provider = "batchsandbox"
    batchsandbox_template_file = "/etc/opensandbox/example.batchsandbox-template.yaml"

    [egress]
    image = "sandbox-registry.cn-zhangjiakou.cr.aliyuncs.com/opensandbox/egress:v1.0.12"
    mode = "dns+nft"
```

Replace `REPLACE_WITH_YOUR_API_KEY` with a strong random string (e.g. `openssl rand -hex 32`). This becomes `OPENSANDBOX_API_KEY` in the opencode-anthropic-server env vars.

**Why these patches are required:**

**Bug 1 — stale `--containerd-socket-path` flag (controller crashes on start):**
The `opensandbox-controller` chart v0.2.0 has `containerdSocketPath: /var/run/containerd/containerd.sock` as a default value. The chart's `deployment.yaml` template passes this as `--containerd-socket-path=<value>` whenever the value is non-empty. The deployed controller binary no longer accepts this flag. Result: the controller pod enters a `CrashLoopBackOff` immediately after install. Fix: set `containerdSocketPath: ""` in values so the flag is not emitted.

**Bug 2 — server requests 4 GiB RAM, t3.medium only has 4 GiB total:**
The `opensandbox-server` chart defaults to `requests.memory: 4Gi` with `replicaCount: 2`. Each t3.medium node has 4 GiB RAM total; ~300–400 MiB is consumed by the OS and daemonsets, leaving ~3.6 GiB allocatable. A 4 GiB request exceeds node capacity and pods remain `Pending` indefinitely. Fix: reduce requests to `512Mi` and limit to `2Gi`.

### 4.3 Install

```bash
kubectl create namespace opensandbox-system

helm install opensandbox charts/opensandbox \
  --namespace opensandbox-system \
  -f opensandbox-values.yaml \
  --wait \
  --timeout 5m
```

### 4.4 Verify

```bash
kubectl -n opensandbox-system get pods
# NAME                                          READY   STATUS    RESTARTS
# opensandbox-controller-manager-xxx            1/1     Running   0
# opensandbox-server-xxx                        1/1     Running   0

kubectl -n opensandbox-system get svc
# NAME                   TYPE        CLUSTER-IP      PORT(S)
# opensandbox-server     ClusterIP   10.100.x.x      80/TCP
```

The server's in-cluster DNS name is `opensandbox-server.opensandbox-system.svc.cluster.local` on port 80. This is the value for `OPENSANDBOX_API_URL` in the next section.

---

## 5. opencode-anthropic-server Deployment

Repo: `https://github.com/LiteLLM-Labs/opencode-anthropic-server`  
Commit deployed: `da6f8719722b6b51ce8063e51be7f3b3c6b4a9b5`

### 5.1 Build and push to ECR

```bash
AWS_ACCOUNT_ID=$(aws sts get-caller-identity --query Account --output text)
AWS_REGION=eu-west-1
ECR_REPO=$AWS_ACCOUNT_ID.dkr.ecr.$AWS_REGION.amazonaws.com/opencode-anthropic-server

# Create the ECR repository (once)
aws ecr create-repository --repository-name opencode-anthropic-server --region $AWS_REGION

# Authenticate Docker to ECR
aws ecr get-login-password --region $AWS_REGION \
  | docker login --username AWS --password-stdin $AWS_ACCOUNT_ID.dkr.ecr.$AWS_REGION.amazonaws.com

# Clone and check out the exact commit
git clone https://github.com/LiteLLM-Labs/opencode-anthropic-server.git
cd opencode-anthropic-server
git checkout da6f8719722b6b51ce8063e51be7f3b3c6b4a9b5

# Build (multi-arch or native; use --platform if building on Apple Silicon for amd64 nodes)
docker build --platform linux/amd64 -t $ECR_REPO:da6f871 -t $ECR_REPO:latest .
docker push $ECR_REPO:da6f871
docker push $ECR_REPO:latest
```

The Dockerfile installs opencode via the official installer (`curl -fsSL https://opencode.ai/install | bash`), installs Node dependencies (`express`, `better-sqlite3`), and runs `node src/index.mjs`. No build secrets required.

### 5.2 Create the namespace and secret

```bash
kubectl create namespace opensandbox-system 2>/dev/null || true

kubectl create secret generic opencode-server-secrets \
  -n opensandbox-system \
  --from-literal=OPENSANDBOX_API_KEY="<the api_key you set in config.toml>" \
  --from-literal=LITELLM_API_KEY="<your litellm gateway key>"
```

### 5.3 Install the EBS CSI driver (required for PVC)

The opencode-anthropic-server persists its SQLite agent store on a PVC (`/data/agents.db`). EKS does not install the EBS CSI driver by default; without it, PVCs using the `ebs.csi.aws.com` provisioner remain `Pending`.

```bash
# Add the EBS CSI driver addon
aws eks create-addon \
  --cluster-name opensandbox \
  --addon-name aws-ebs-csi-driver \
  --region eu-west-1

# Wait for it to become active
aws eks wait addon-active \
  --cluster-name opensandbox \
  --addon-name aws-ebs-csi-driver \
  --region eu-west-1
```

Create the `gp3` StorageClass (faster and cheaper than `gp2`):

```bash
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
  fsType: ext4
volumeBindingMode: WaitForFirstConsumer
reclaimPolicy: Retain
EOF
```

### 5.4 Apply the Kubernetes manifests

Save as `opencode-server.yaml`. Replace `$ECR_REPO` with your full ECR URI and `$LITELLM_BASE_URL` with your gateway URL before applying.

```yaml
---
apiVersion: v1
kind: PersistentVolumeClaim
metadata:
  name: opencode-data
  namespace: opensandbox-system
spec:
  accessModes:
    - ReadWriteOnce
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
      containers:
        - name: server
          image: <YOUR_ECR_REPO>:da6f871
          ports:
            - containerPort: 8080
          env:
            - name: PORT
              value: "8080"
            - name: DB_PATH
              value: /data/agents.db
            - name: WORKDIR
              value: /tmp/opencode-workspace
            # OpenSandbox wiring — routes agent execution into sandbox containers
            - name: OPENSANDBOX_API_URL
              value: "http://opensandbox-server.opensandbox-system.svc.cluster.local"
            - name: OPENSANDBOX_API_KEY
              valueFrom:
                secretKeyRef:
                  name: opencode-server-secrets
                  key: OPENSANDBOX_API_KEY
            # LiteLLM gateway — routes opencode model calls through your gateway
            - name: LITELLM_BASE_URL
              value: "https://<YOUR_LITELLM_GATEWAY>/v1"
            - name: LITELLM_API_KEY
              valueFrom:
                secretKeyRef:
                  name: opencode-server-secrets
                  key: LITELLM_API_KEY
            - name: LITELLM_MODELS
              value: "claude-sonnet-4-5,gpt-4o"
          volumeMounts:
            - name: data
              mountPath: /data
          resources:
            requests:
              cpu: "250m"
              memory: "512Mi"
            limits:
              cpu: "1"
              memory: "1Gi"
          livenessProbe:
            httpGet:
              path: /health
              port: 8080
            initialDelaySeconds: 30
            periodSeconds: 15
          readinessProbe:
            httpGet:
              path: /health
              port: 8080
            initialDelaySeconds: 10
            periodSeconds: 10
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
spec:
  type: LoadBalancer
  selector:
    app: opencode-anthropic-server
  ports:
    - name: http
      port: 80
      targetPort: 8080
```

Apply:

```bash
kubectl apply -f opencode-server.yaml
```

Wait for the pod to be ready and the LoadBalancer to provision an external hostname:

```bash
kubectl -n opensandbox-system rollout status deployment/opencode-anthropic-server

kubectl -n opensandbox-system get svc opencode-anthropic-server
# NAME                        TYPE           CLUSTER-IP     EXTERNAL-IP           PORT(S)
# opencode-anthropic-server   LoadBalancer   10.100.x.x     <ELB hostname>        80:3xxxx/TCP
```

The `EXTERNAL-IP` field shows the AWS ELB DNS name. This can take 60–90 seconds to appear.

---

## 6. Wiring Them Together

### 6.1 Service discovery

All components run in `opensandbox-system`. In-cluster DNS resolves services as:

| Service | In-cluster URL | Purpose |
|---------|---------------|---------|
| `opensandbox-server` | `http://opensandbox-server.opensandbox-system.svc.cluster.local` | OpenSandbox API (sandbox create/exec) |
| `opencode-anthropic-server` | `http://opencode-anthropic-server.opensandbox-system.svc.cluster.local` | Anthropic Managed Agents API (internal) |
| `opencode-anthropic-server` LoadBalancer | `http://<ELB hostname>` | Anthropic Managed Agents API (external) |

### 6.2 Environment variable reference

| Variable | Set on | Value | Effect |
|----------|--------|-------|--------|
| `OPENSANDBOX_API_URL` | opencode-anthropic-server | `http://opensandbox-server.opensandbox-system.svc.cluster.local` | Enables sandboxed execution; agent bash/file ops routed here instead of running on the server pod |
| `OPENSANDBOX_API_KEY` | opencode-anthropic-server (from secret) | matches `api_key` in OpenSandbox `config.toml` | Authenticates requests to the OpenSandbox server |
| `LITELLM_BASE_URL` | opencode-anthropic-server | `https://<gateway>/v1` | Configures opencode's `litellm` provider; agents use `litellm/<model>` |
| `LITELLM_API_KEY` | opencode-anthropic-server (from secret) | your gateway key | Auth for LiteLLM gateway calls |
| `LITELLM_MODELS` | opencode-anthropic-server | `claude-sonnet-4-5,gpt-4o` | Comma-separated models registered under the `litellm` provider |
| `api_key` (config.toml) | OpenSandbox server | strong random string | Server refuses to start with an empty key unless `OPENSANDBOX_INSECURE_SERVER=YES` is set |

### 6.3 How sandboxed execution works

When `OPENSANDBOX_API_URL` is set, the opencode-anthropic-server:

1. Denies opencode's native `bash` and `edit` tools.
2. Injects a sandbox-exec MCP server (via `writeSandboxConfig`) that exposes `sandbox_exec`, `sandbox_read_file`, `sandbox_write_file` tools.
3. The MCP server communicates with OpenSandbox over raw HTTP (no SDK):
   - `POST /v1/sandboxes` — creates a sandbox container.
   - `GET /v1/sandboxes/{id}/endpoints/44772?use_server_proxy=true` — resolves the execd endpoint.
   - `POST {execd}/command` — runs commands (SSE stream of stdout/stderr events).
   - `GET {execd}/files/download?path=...` / `POST {execd}/files/upload` — file I/O.

### 6.4 LAP SDK client configuration

Point the SDK at the LoadBalancer external hostname:

```rust
let lap = Lap::new(LapConfig {
    anthropic_api_key: Some("any-key".into()),      // accepted loosely
    anthropic_base_url: "http://<ELB hostname>".into(),
    ..LapConfig::default()
});
```

Use `litellm/<model>` as the model name in agent creation when routing through the LiteLLM gateway:

```rust
model: AgentModel::from("litellm/claude-sonnet-4-5"),
```

---

## 7. Verification

### 7.1 Check OpenSandbox is healthy

```bash
OPENSANDBOX_URL=http://opensandbox-server.opensandbox-system.svc.cluster.local

# From inside the cluster (exec into any pod):
kubectl -n opensandbox-system exec -it deployment/opencode-anthropic-server -- \
  curl -s -H "OPEN-SANDBOX-API-KEY: <your-key>" \
  http://opensandbox-server.opensandbox-system.svc.cluster.local/v1/sandboxes
# -> {"items":[],"total":0} or similar
```

### 7.2 Check opencode-anthropic-server health

```bash
EXTERNAL_IP=$(kubectl -n opensandbox-system get svc opencode-anthropic-server \
  -o jsonpath='{.status.loadBalancer.ingress[0].hostname}')

curl -s http://$EXTERNAL_IP/health
# -> {"ok":true,"opencode":true}
# Note: opencode boots in the background; "opencode":false for ~10-20s after pod start is normal.
```

### 7.3 Full smoke test (create agent → session → message → stream)

The repo ships a smoke test script. Run it against the live endpoint:

```bash
BASE=http://$EXTERNAL_IP \
MODEL=litellm/claude-sonnet-4-5 \
  ./scripts/smoke.sh
```

Expected output:

```
=== 1. GET /health ===
{"ok":true,"opencode":true}

=== 2. POST /v1/agents ===
{"id":"agt_...","type":"agent","name":"Smoke Test", ...}
agent id: agt_...

=== 3. GET /v1/agents/agt_... ===
{"id":"agt_...","type":"agent", ...}

=== 4. POST /v1/environments ===
{"id":"env_...","type":"environment", ...}

=== 5. POST /v1/sessions ===
{"id":"ses_...","type":"session","status":"running", ...}
session id: ses_...

=== 6. GET /v1/sessions/ses_.../events/stream (background SSE)
(opening stream in background)

=== 7. POST /v1/sessions/ses_.../events ===
{"ok":true}

=== 8. captured SSE events ===
event: session.status_running
data: {...}

event: agent.message
data: {"content":[{"type":"text","text":"Hello there, friend!"}], ...}

event: session.status_idle
data: {...}
```

### 7.4 Verify sandboxed execution is active

Check the server startup logs for the sandbox activation message:

```bash
kubectl -n opensandbox-system logs deployment/opencode-anthropic-server | grep sandbox
# [boot] sandbox execution enabled (opensandbox) — bash/edit denied, routed to sandbox MCP
```

If this line is absent, `OPENSANDBOX_API_URL` is not set correctly. If it shows an error instead, check that the `OPENSANDBOX_API_KEY` matches the `api_key` in the OpenSandbox `config.toml`.

### 7.5 Inspect sandbox container creation

When a session runs a command, OpenSandbox creates a BatchSandbox pod in the `opensandbox` namespace:

```bash
kubectl -n opensandbox get batchsandbox
kubectl -n opensandbox get pods
```

These pods appear during active agent turns and are cleaned up by the controller.

---

## Appendix: Troubleshooting

**Controller pod in CrashLoopBackOff after helm install:**
Check logs with `kubectl -n opensandbox-system logs deployment/opensandbox-controller-manager`. If you see `unknown flag: --containerd-socket-path`, the v0.2.0 chart bug is biting you. Apply the values patch from section 4.2 and run `helm upgrade opensandbox charts/opensandbox -n opensandbox-system -f opensandbox-values.yaml`.

**OpenSandbox server pod Pending:**
`kubectl -n opensandbox-system describe pod <server-pod>` will show an `Insufficient memory` event if the default 4 GiB request exceeds node capacity. Apply the resource patch from section 4.2.

**OpenSandbox server exits immediately / "server refused to start":**
The server refuses to start if `api_key` in `config.toml` is an empty string, unless the environment variable `OPENSANDBOX_INSECURE_SERVER=YES` is set. Provide a non-empty `api_key` in the `configToml` values override.

**PVC stuck in Pending:**
The EBS CSI driver addon is not installed or the `gp3` StorageClass was not created. Run the addon install commands in section 5.3. Verify with `kubectl get storageclass`.

**LoadBalancer EXTERNAL-IP stuck in `<pending>`:**
This takes 60–90s after `kubectl apply`. If it persists beyond 5 minutes, check that the EKS cluster's VPC subnets are tagged correctly for ELB provisioning:
- Public subnets: `kubernetes.io/role/elb = 1`
- Private subnets: `kubernetes.io/role/internal-elb = 1`

eksctl sets these automatically; if you modified the VPC manually they may be missing.

**`opencode: false` in `/health` after several minutes:**
The child opencode process failed to start. Check container logs:
```bash
kubectl -n opensandbox-system logs deployment/opencode-anthropic-server --tail=50
```
Common cause: opencode binary not on `PATH` inside the container. The Dockerfile installs it to `/root/.opencode/bin` and sets `ENV PATH="/root/.opencode/bin:$PATH"` — verify the image was built from the correct Dockerfile.

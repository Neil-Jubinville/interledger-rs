
# Helm

 This folder contains helm scripts that will allow you to deploy and expose ports for multiple releases of an ILP rust node implementation.  Nodes are standalone and each have their own redis store.   

 

 ## Pre-Requistes:

* Previously intsalled Minikube or ensured access to a Kubernetes platform for where Helm will deploy to.
* Previously have run docker build -t neil/interledger-rs in an accessible docker repository from your K8 infrastructure.  
* Helm and Tiller installed.

## Running the Deployment 

```
helm install .
```

You Should see output like this:


```
LAST DEPLOYED: Wed Jul 24 16:55:07 2019
NAMESPACE: default
STATUS: DEPLOYED

RESOURCES:
==> v1/ConfigMap
NAME                 AGE
rust-ilpnode-config  0s

==> v1/Service
ilp-rust-service  0s

==> v1/Deployment
ilp-rust-node  0s

==> v1/Pod(related)

NAME                            READY  STATUS       RESTARTS  AGE
ilp-rust-node-76dbdb99ff-kzs4b  1/1    Ready        0         0s

```



TODO:

+ Build charts for abstracted ILP micro services vs all-in one nodes.
+ Build a Pod spec for shared underlying account store.
+ Abstract out sensitve fields into Secrets and others into the values.yaml.
+ Test the ephemeral aspects of pods dying in live accounting transmissions.
uniform sampler2D s0;

static const float4 c2 = float4(8.0, 9.0, 0.189999998, 0.150000006);
static const float4 c3 = float4(0.0, 0.170000002, 2.0, 3.0);
static const float4 c4 = float4(0.129999995, 0.109999999, 0.090000004, 0.07);
static const float4 c5 = float4(4.0, 5.0, 6.0, 7.0);
static const float4 c6 = float4(0.050000001, 0.029999999, 0.01, 0.0);

struct PS_INPUT {
    float4 v0 : COLOR0;
    float4 t0 : TEXCOORD0;
};

struct PS_OUTPUT {
    float4 oC0 : COLOR0;
};

PS_OUTPUT main(PS_INPUT input) {
    PS_OUTPUT output;
    output.oC0 = float4(0.0, 0.0, 0.0, 0.0);
    float4 r0 = float4(0.0, 0.0, 0.0, 0.0);
    float4 r1 = float4(0.0, 0.0, 0.0, 0.0);
    float4 r2 = float4(0.0, 0.0, 0.0, 0.0);
    float4 r3 = float4(0.0, 0.0, 0.0, 0.0);
    float4 r4 = float4(0.0, 0.0, 0.0, 0.0);
    float4 r5 = float4(0.0, 0.0, 0.0, 0.0);
    float4 r6 = float4(0.0, 0.0, 0.0, 0.0);
    float4 r7 = float4(0.0, 0.0, 0.0, 0.0);
    float4 r8 = float4(0.0, 0.0, 0.0, 0.0);
    float4 r9 = float4(0.0, 0.0, 0.0, 0.0);

    r0.x = (-(input.t0.x) + c0.z);
    r0.y = (-(input.t0.y) + c0.w);
    r0.z = (dot(r0.xy, r0.xy) + c3.x);
    r0.z = rsqrt(r0.z);
    r0.xy = (r0.xy * r0.zz);
    r0.z = (1.0 / r0.z);
    r0.xy = (r0.xy * c0.xx);
    r0.z = (r0.z * c1.x);
    r0.z = (r0.z * c1.y);
    r1.xy = (r0.xy * r0.zz + input.t0.xy);
    r0.xy = (r0.xy * r0.zz);
    r2.xy = (r0.xy * c3.zz + input.t0.xy);
    r3.xy = (r0.xy * c3.ww + input.t0.xy);
    r4.xy = (r0.xy * c5.xx + input.t0.xy);
    r5.xy = (r0.xy * c5.yy + input.t0.xy);
    r6.xy = (r0.xy * c5.zz + input.t0.xy);
    r7.xy = (r0.xy * c5.ww + input.t0.xy);
    r8.xy = (r0.xy * c2.xx + input.t0.xy);
    r0.xy = (r0.xy * c2.yy + input.t0.xy);
    r1 = tex2D(s0, r1.xy);
    r9 = tex2D(s0, input.t0.xy);
    r2 = tex2D(s0, r2.xy);
    r3 = tex2D(s0, r3.xy);
    r4 = tex2D(s0, r4.xy);
    r5 = tex2D(s0, r5.xy);
    r6 = tex2D(s0, r6.xy);
    r7 = tex2D(s0, r7.xy);
    r8 = tex2D(s0, r8.xy);
    r0 = tex2D(s0, r0.xy);
    r1.xyz = (r1.xyz * c3.yyy);
    r1.xyz = (r9.xyz * c2.zzz + r1.xyz);
    r1.xyz = (r2.xyz * c2.www + r1.xyz);
    r1.xyz = (r3.xyz * c4.xxx + r1.xyz);
    r1.xyz = (r4.xyz * c4.yyy + r1.xyz);
    r1.xyz = (r5.xyz * c4.zzz + r1.xyz);
    r1.xyz = (r6.xyz * c4.www + r1.xyz);
    r1.xyz = (r7.xyz * c6.xxx + r1.xyz);
    r1.xyz = (r8.xyz * c6.yyy + r1.xyz);
    r0.xyz = (r0.xyz * c6.zzz + r1.xyz);
    r0.w = input.v0.w;
    output.oC0 = r0;
    return output;
}

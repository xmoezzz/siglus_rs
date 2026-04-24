uniform sampler2D s0;
uniform sampler2D s1;

static const float4 c13 = float4(0.0, 1.0, 3.140000105, 0.0);
static const float4 c14 = float4(0.159154937, 0.5, 6.283185482, -3.141592741);
static const float4 c15 = float4(-0.00000155, -0.000021701, 0.002604167, 0.000260417);
static const float4 c16 = float4(-0.020833334, -0.125, 1.0, 0.5);

struct PS_INPUT {
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

    r0.w = (input.t0.y * c12.x);
    r0.x = frac(r0.w);
    r0.y = (r0.w + -(r0.x));
    r0.x = (-(r0.x) >= 0 ? c13.y : c13.x);
    r0.z = (r0.w >= 0 ? c13.y : c13.x);
    r0.x = (r0.z * r0.x + r0.y);
    r0.x = (r0.x + -(c0.x));
    r0.x = (r0.x * c1.x);
    r0.x = (r0.x * c12.y);
    r0.z = c13.z;
    r0.x = (r0.x * r0.z + c6.x);
    r0.x = (r0.x * c14.x + c14.y);
    r0.x = frac(r0.x);
    r0.x = (r0.x * c14.z + c14.w);
    r1.y = sin(r0.x);
    r0.x = (r1.y * c9.x + input.t0.x);
    r0.z = (-(r0.x) + c13.y);
    r0.z = (r0.z >= 0 ? c13.y : c13.x);
    r0.w = (r0.x >= 0 ? c13.y : c13.x);
    r0.z = (r0.z + r0.w);
    r0.z = (-(r0.z) >= 0 ? c13.y : c13.x);
    r0.w = (input.t0.y >= 0 ? c13.y : c13.x);
    r0.z = (r0.z + r0.w);
    r0.z = (-(r0.z) >= 0 ? c13.y : c13.x);
    r0.w = (-(input.t0.y) + c13.y);
    r0.w = (r0.w >= 0 ? c13.y : c13.x);
    r0.z = (r0.z + r0.w);
    r0.y = input.t0.y;
    r1 = tex2D(s0, r0.xy);
    r2 = tex2D(s1, r0.xy);
    r1 = (-(r0.zzzz) >= 0 ? c13.xxxx : r1);
    r0 = (-(r0.zzzz) >= 0 ? c13.xxxx : r2);
    r1 = (r1 * c11.xxxx);
    r0 = (r0 * c10.xxxx + r1);
    output.oC0 = r0;
    return output;
}
